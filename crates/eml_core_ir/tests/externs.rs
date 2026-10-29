//! extern の表の行の Repr と、std の宣言の型の Repr の照らし合わせ (docs/spec/core-ir.md)。行の名前と矢印の数は
//! `eml_hir` の結合テストが照らし合わせる。型から Repr を決める規則は translate にあるので、ここで確かめる。
//! extern が作る値のタグと、translate が作る配置も照らし合わせる。

use eml_core_ir::boxed_repr;
use eml_extern::{Extern, ExternType, Mark, OpRet, Purity, Repr, RuntimeEntry};
use eml_hir::{Function, FunctionKind, MethodImpl, TypeDefId, ValueItem};
use eml_lir::{FALSE, TRUE, TUPLE};
use eml_test_support::Checked;
use eml_types::{Lin, RowTail, Substitution, TypeId, TypeKind, TypeStore};

/// 型の行ごとに、その型の値をそのまま返す関数を置く。型検査がその型に与える型を、関数の引数の型から読む。標準
/// ライブラリは使うモジュールだけを読み込むので、すべてのモジュールを import する (`probes`)。
const PROBES: &str = "\
probe_int : Int -> Int
probe_int x = x

probe_char : Char -> Char
probe_char c = c

probe_byte : Byte -> Byte
probe_byte b = b

probe_string : String -> String
probe_string s = s

probe_bytes : Bytes -> Bytes
probe_bytes b = b

probe_float : Float -> Float
probe_float x = x

probe_unit : Unit -> Unit
probe_unit u = u

probe_file : FileIn.FileIn -> FileIn.FileIn
probe_file f = f

probe_file_out : FileOut.FileOut -> FileOut.FileOut
probe_file_out f = f

probe_stdin : Stdin.Stdin -> Stdin.Stdin
probe_stdin h = h

probe_stdout : Stdout.Stdout -> Stdout.Stdout
probe_stdout h = h

probe_stderr : Stderr.Stderr -> Stderr.Stderr
probe_stderr h = h

probe_child : Proc.Child -> Proc.Child
probe_child c = c

probe_array : Array Int -> Array Int
probe_array a = a

probe_mut_array : MutArray Int -> MutArray Int
probe_mut_array a = a

probe_cell : Async.Cell -> Async.Cell
probe_cell c = c

probe_task : Async.Task Int -> Async.Task Int
probe_task t = t

probe_scope : Async.Scope -> Async.Scope
probe_scope s = s

probe_chan : Chan.Chan Int -> Chan.Chan Int
probe_chan c = c

probe_counter : Atomic.Counter -> Atomic.Counter
probe_counter c = c

probe_op : Async.OpId -> Async.OpId
probe_op o = o

probe_tcp_listener : Net.TcpListener -> Net.TcpListener
probe_tcp_listener l = l

probe_tcp_stream : Net.TcpStream -> Net.TcpStream
probe_tcp_stream s = s

probe_panic : Unit -> <Panic> Int
probe_panic () = panic \"x\"

probe_try_panic : Unit -> Result String Int
probe_try_panic () = try_panic probe_panic

main : Unit -> <IO> Unit
main () = println \"x\"
";

/// 型の行を読む `PROBES` の関数。`_` の腕を書かないので、型の行を足すとここがコンパイルできなくなる。
fn probe(ty: ExternType) -> &'static str {
    match ty {
        ExternType::Int => "probe_int",
        ExternType::Float => "probe_float",
        ExternType::Char => "probe_char",
        ExternType::Byte => "probe_byte",
        ExternType::String => "probe_string",
        ExternType::Bytes => "probe_bytes",
        ExternType::Unit => "probe_unit",
        ExternType::FileIn => "probe_file",
        ExternType::FileOut => "probe_file_out",
        ExternType::Stdin => "probe_stdin",
        ExternType::Stdout => "probe_stdout",
        ExternType::Stderr => "probe_stderr",
        ExternType::Child => "probe_child",
        ExternType::Array => "probe_array",
        ExternType::MutArray => "probe_mut_array",
        ExternType::Cell => "probe_cell",
        ExternType::Task => "probe_task",
        ExternType::Scope => "probe_scope",
        ExternType::Chan => "probe_chan",
        ExternType::Counter => "probe_counter",
        ExternType::OpId => "probe_op",
        ExternType::NetTcpListener => "probe_tcp_listener",
        ExternType::NetTcpStream => "probe_tcp_stream",
    }
}

/// `PROBES` の前に、Prelude のほかのすべての標準ライブラリのモジュールの import を置く。
fn probes() -> String {
    let imports: String = eml_hir::STD
        .iter()
        .filter_map(|(file, _)| file.strip_suffix(".em"))
        .filter(|&name| name != "Prelude")
        .map(|name| format!("import Std.{}\n", name.replace('/', ".")))
        .collect();
    format!("{imports}\n{PROBES}")
}

fn checked() -> Checked {
    let checked = eml_test_support::check(&probes());
    assert!(
        checked.diagnostics.is_empty(),
        "{}",
        eml_test_support::short_text(checked.files(), &checked.diagnostics)
    );
    checked
}

/// `wanted` に合う関数の宣言の型。
fn declared(checked: &Checked, wanted: impl Fn(&Function) -> bool) -> TypeId {
    let (id, _) = checked
        .program
        .functions()
        .find(|(_, function)| wanted(function))
        .expect("the function is declared");
    checked.typed.decls[&ValueItem::Function(id)].ty
}

/// 行 `e` の std のシグネチャの型。extern の関数なら宣言の型で、instance の `extern` で結んだメソッドなら、クラスの
/// メソッドの宣言の型のクラスの型変数を instance の頭の型に置き換えた型である
/// (docs/spec/declarations.md の「`extern`」)。置き換えた型は
/// `types` に足す。
fn row_type(checked: &Checked, types: &mut TypeStore, e: Extern) -> TypeId {
    let program = &checked.program;
    if let Some((id, _)) = program
        .functions()
        .find(|(_, function)| function.kind == FunctionKind::Extern(Some(e)))
    {
        return checked.typed.decls[&ValueItem::Function(id)].ty;
    }
    let (instance, method) = program
        .instances()
        .find_map(|(_, instance)| {
            let (method, _) = instance
                .methods
                .iter()
                .find(|&&(_, implementation)| implementation == MethodImpl::Extern(e))?;
            Some((instance, *method))
        })
        .unwrap_or_else(|| panic!("`{}` is bound in std", e.row().name));
    let head = head_type(checked, instance.head);
    let var = program[instance.class].var.clone();
    let declared = checked.typed.decls[&ValueItem::Method(method)].ty;
    types.substitute(declared, &mut Substitution::partial([(var, head)]))
}

/// 行 `e` を instance の `extern` が結んでいるか。どの行も std のちょうど1か所から指されるので、extern の関数が結んで
/// いなければ instance の `extern` が結んでいる。
fn method_bound(checked: &Checked, e: Extern) -> bool {
    !checked
        .program
        .functions()
        .any(|(_, function)| function.kind == FunctionKind::Extern(Some(e)))
}

/// 型引数のない型 `head` を指す、型の表の型。テストは型の表に型を作れないので、宣言の型の中から探す。
fn head_type(checked: &Checked, head: TypeDefId) -> TypeId {
    let types = &checked.typed.types;
    let mut pending: Vec<TypeId> = checked.typed.decls.values().map(|decl| decl.ty).collect();
    while let Some(ty) = pending.pop() {
        match types.kind(ty) {
            TypeKind::Con { id, args, rows: _ } if *id == head && args.is_empty() => return ty,
            TypeKind::Con {
                id: _,
                args,
                rows: _,
            } => pending.extend(args),
            TypeKind::Fn { param, ret, .. } => pending.extend([*param, *ret]),
            TypeKind::Tuple(elements) => pending.extend(elements),
            TypeKind::Rigid(_)
            | TypeKind::OpVar(_, _)
            | TypeKind::Flexible(_)
            | TypeKind::Error => {}
        }
    }
    panic!(
        "`{}` appears in a declared type",
        checked.program[head].name
    )
}

/// 行の結果と比べる std の型。`with_arg` の行の宣言の型は結果と引数の組なので、組の最初の要素である。2つ目の要素は
/// 引数 `k` の型と同じである (docs/spec/core-ir.md の「組を返す extern」)。
fn row_result(
    types: &TypeStore,
    name: &str,
    with_arg: Option<usize>,
    params: &[TypeId],
    ty: TypeId,
) -> TypeId {
    let Some(k) = with_arg else {
        return ty;
    };
    let TypeKind::Tuple(elements) = types.kind(ty) else {
        panic!("`{name}` returns a pair");
    };
    assert_eq!(elements.len(), 2, "{name}");
    assert_eq!(elements[1], params[k], "{name}");
    elements[0]
}

#[test]
fn every_extern_function_row_has_the_reprs_of_its_std_signature() {
    // 行の Repr は引数の型と結果の型から比べる。型変数の位置は `tobj` である。戻らない extern の結果は、どの型にも
    // なる型変数である (docs/spec/lir.md の「戻らない extern」)
    let checked = checked();
    let mut store = checked.typed.types.clone();
    for &e in Extern::ALL {
        let row = e.row();
        let mut ty = row_type(&checked, &mut store, e);
        let types = &store;
        let mut params = Vec::new();
        let mut param_types = Vec::new();
        for _ in row.params {
            let TypeKind::Fn { param, ret, .. } = types.kind(ty) else {
                panic!("`{}` has an arrow per parameter", row.name);
            };
            params.push(boxed_repr(types, *param, &checked.program));
            param_types.push(*param);
            ty = *ret;
        }
        assert_eq!(params, row.params, "{}", row.name);
        assert_eq!(row.borrowed.len(), row.params.len(), "{}", row.name);
        match row.ret {
            OpRet::Repr(ret) => {
                let result = row_result(types, row.name, row.with_arg, &param_types, ty);
                assert_eq!(
                    boxed_repr(types, result, &checked.program),
                    ret,
                    "{}",
                    row.name
                );
            }
            OpRet::Never => assert!(
                matches!(types.kind(ty), TypeKind::Rigid(_)),
                "`{}` never returns, so its result is a type variable",
                row.name
            ),
        }
    }
}

/// 表の値そのもの。借りる引数を持つ行は次の表の行だけで、組を返すのは `Std.MutArray` の2つの行、`Std.Async.task_cell`、
/// `Std.Async.op_ready`、
/// `Std.Net` の `op_listener`、`op_stream`、`op_outcome`、`local_port`、`peer_addr` と、`Std.FileIn`、`Std.Stdin`、
/// `Std.Proc` のそれぞれ5つの読みの行だけである。`Std.FileOut`、`Std.Stdout`、`Std.Stderr`、`Std.Proc` の
/// 書き込みの行は、データを借りてハンドルを返すので、組を返さない。
#[test]
fn only_the_listed_rows_borrow_or_return_a_pair() {
    for &e in Extern::ALL {
        let row = e.row();
        let expected: (&[bool], Option<usize>) = match row.name {
            "Std.Array.length" => (&[true], None),
            "Std.Array.get" => (&[true, false], None),
            "Std.MutArray.length" => (&[true], Some(0)),
            "Std.MutArray.read" => (&[true, false], Some(0)),
            "Std.Async.task_cell" => (&[true], Some(0)),
            "Std.Async.op_ready" => (&[true, false], Some(0)),
            "Std.Net.op_listener"
            | "Std.Net.op_stream"
            | "Std.Net.op_outcome"
            | "Std.Net.local_port"
            | "Std.Net.peer_addr" => (&[true], Some(0)),
            "Std.FileIn.read_all"
            | "Std.FileIn.read_line_raw"
            | "Std.FileIn.read_all_bytes"
            | "Std.FileIn.read_line_bytes_raw"
            | "Std.Stdin.read_all"
            | "Std.Stdin.read_line_raw"
            | "Std.Stdin.read_all_bytes"
            | "Std.Stdin.read_line_bytes_raw"
            | "Std.Proc.read_all"
            | "Std.Proc.read_line_raw"
            | "Std.Proc.read_all_bytes"
            | "Std.Proc.read_line_bytes_raw" => (&[true], Some(0)),
            "Std.FileIn.read_chunk" | "Std.Stdin.read_chunk" | "Std.Proc.read_chunk" => {
                (&[false, true], Some(1))
            }
            "Std.String.byte_length" | "Prelude.Show String.show" => (&[true], None),
            "Std.String.byte_at" | "Std.String.char_at" => (&[false, true], None),
            "Std.String.slice" => (&[false, false, true], None),
            "Std.String.find_from" => (&[true, false, true], None),
            "Prelude.Eq String.==" | "Prelude.Eq String.!=" | "Prelude.Ord String.compare" => {
                (&[true, true], None)
            }
            "Std.Float.can_parse" | "Std.Float.parse_raw" => (&[true], None),
            "Std.Bytes.length" | "Prelude.bytes_length" => (&[true], None),
            "Std.Bytes.get" | "Prelude.bytes_get" => (&[false, true], None),
            "Std.Bytes.slice" | "Std.Bytes.find_byte_from" => (&[false, false, true], None),
            "Std.Bytes.append_slice" => (&[false, false, true, false], None),
            "Std.Bytes.find_from" => (&[true, false, true], None),
            "Prelude.Eq Bytes.==" | "Prelude.Eq Bytes.!=" | "Prelude.Ord Bytes.compare" => {
                (&[true, true], None)
            }
            "Std.FileOut.write"
            | "Std.FileOut.write_bytes"
            | "Std.FileOut.write_line"
            | "Std.Stdout.write"
            | "Std.Stdout.write_bytes"
            | "Std.Stdout.write_line"
            | "Std.Stderr.write"
            | "Std.Stderr.write_bytes"
            | "Std.Stderr.write_line"
            | "Std.Proc.write"
            | "Std.Proc.write_bytes"
            | "Std.Proc.write_line" => (&[true, false], None),
            _ => {
                assert!(!row.borrowed.contains(&true), "{}", row.name);
                assert_eq!(row.with_arg, None, "{}", row.name);
                continue;
            }
        };
        assert_eq!((row.borrowed, row.with_arg), expected, "{}", row.name);
    }
}

#[test]
fn faulting_rows_carry_panic() {
    // 行の `purity` は、std の型の最後の外側の矢印の row と合う。`Panics` は `Panic` を起こし `IO` を起こさない。
    // `Effectful` は `IO` を起こし、`Std.Env.arg` のように `Panic` も起こしうる。`Pure` は何も起こさない。ただし
    // instance の `extern` で結んだ `Pure` の行は、クラスのメソッドの型が書く `Panic` だけを持ってよい (`Num Float`)
    // (docs/spec/effects.md の「組み込みの `Panic`」)。外側の矢印の row が開いてよいのは、末尾が関数型の引数の row の
    // 末尾でもあるときだけである。コールバックのエフェクトを呼んだ側へ通す高階の extern (`Std.Par.par`) の形である
    let checked = checked();
    let program = &checked.program;
    let (io, panic) = (program.io(), program.panic());
    let mut store = checked.typed.types.clone();
    for &e in Extern::ALL {
        let row = e.row();
        let mut ty = row_type(&checked, &mut store, e);
        let callback_tails = callback_tails(&store, ty, row.params.len());
        let mut labels = Vec::new();
        for _ in row.params {
            let TypeKind::Fn {
                effects, tail, ret, ..
            } = store.kind(ty)
            else {
                panic!("`{}` has an arrow per parameter", row.name);
            };
            assert!(
                tail.as_ref()
                    .is_none_or(|tail| callback_tails.contains(tail)),
                "{}",
                row.name
            );
            labels = effects.iter().map(|label| label.id).collect();
            ty = *ret;
        }
        match row.purity {
            Purity::Pure if method_bound(&checked, e) => {
                assert!(labels.is_empty() || labels == [panic], "{}", row.name)
            }
            Purity::Pure => assert!(labels.is_empty(), "{}", row.name),
            Purity::Panics => assert_eq!(labels, [panic], "{}", row.name),
            Purity::Effectful => assert!(labels.contains(&io), "{}", row.name),
        }
    }
    // `panic` は Prelude の `pub` の extern の関数で、`PROBES` が名前で呼べる
    let (_, function) = program
        .functions()
        .find(|(_, function)| function.kind == FunctionKind::Extern(Some(Extern::Panic)))
        .expect("`Prelude.panic` is declared");
    assert_eq!(function.name, "panic");
    assert_eq!(Extern::Panic.row().purity, Purity::Panics);
    assert_eq!(Extern::Panic.row().ret, OpRet::Never);
}

/// 一番外側の `arity` 個の矢印の引数のうち、関数型のものの矢印の row の末尾。
fn callback_tails(types: &TypeStore, mut ty: TypeId, arity: usize) -> Vec<RowTail> {
    let mut tails = Vec::new();
    for _ in 0..arity {
        let TypeKind::Fn { param, ret, .. } = types.kind(ty) else {
            break;
        };
        let mut arrow = *param;
        while let TypeKind::Fn { tail, ret, .. } = types.kind(arrow) {
            tails.extend(tail.clone());
            arrow = *ret;
        }
        ty = *ret;
    }
    tails
}

#[test]
fn every_extern_function_row_has_the_marks_of_its_std_signature() {
    let checked = checked();
    let mut store = checked.typed.types.clone();
    for &e in Extern::ALL {
        let row = e.row();
        let mut ty = row_type(&checked, &mut store, e);
        let types = &store;
        assert_eq!(row.param_marks.len(), row.params.len(), "{}", row.name);
        let mut params = Vec::new();
        let mut param_types = Vec::new();
        for _ in row.params {
            let TypeKind::Fn { param, ret, .. } = types.kind(ty) else {
                panic!("`{}` has an arrow per parameter", row.name);
            };
            params.push(declared_mark(types, *param, &checked.program));
            param_types.push(*param);
            ty = *ret;
        }
        for (index, (&declared, &mark)) in params.iter().zip(row.param_marks).enumerate() {
            assert!(
                mark_fits(declared, mark),
                "{} parameter {index}: declared {declared:?}, row {mark:?}",
                row.name
            );
        }
        if let OpRet::Repr(_) = row.ret {
            let result = row_result(types, row.name, row.with_arg, &param_types, ty);
            let declared = declared_mark(types, result, &checked.program);
            assert!(
                mark_fits(declared, row.ret_mark),
                "{} result: declared {declared:?}, row {:?}",
                row.name,
                row.ret_mark
            );
        }
    }
}

/// 宣言の型 `ty` から読む印。Repr が RC の対象でない値は、型の Kind によらず `Unr` である (`Std.Async.OpId` の
/// 即値。docs/spec/core-ir.md の「translate が決める印」)。
fn declared_mark(types: &TypeStore, ty: TypeId, program: &eml_hir::Program) -> Mark {
    if !boxed_repr(types, ty, program).is_rc() {
        return Mark::Unr;
    }
    mark_of(types.linearity_or_poly(ty))
}

/// 型の Kind を行の印に写す。関数でない引数の型変数は、extern のスキームが `Unr` に固定する。関数型の引数
/// (`Std.Par.par` の子) の矢印の Kind は固定しないので、どの印の閉包も渡せる。結果だけに現れる型変数は固定しないが、
/// 結果がその型変数に依存する行 (`MutArray.empty` の `MutArray a`) は `Lin` である。固定しない位置は `Poly` の行と
/// 比べる (docs/spec/core-ir.md の「表の印の列」)。
fn mark_of(lin: Lin) -> Mark {
    match lin {
        Lin::Unr => Mark::Unr,
        Lin::Lin => Mark::Lin,
        Lin::Poly => Mark::Poly,
        Lin::Var(_) => panic!("an extern row has no kind variable"),
    }
}

/// 宣言の型から読んだ印と行の印が合うか。型変数に依存する型は `Poly` と読めるが、スキームが `Unr` に固定する位置の
/// 行は `Unr` で、固定しない位置の行は `Poly` なので、`Poly` はどちらの行とも合う。固定するかどうかは
/// 宣言の型から読めないので、固定しない行は `only_the_listed_rows_carry_a_lin_or_poly_mark` が列挙する。
fn mark_fits(declared: Mark, row: Mark) -> bool {
    declared == row || (declared == Mark::Poly && row == Mark::Unr)
}

/// 表の値そのもの。`FileIn` の7つ、`Stdin` の7つ、`Stdout` の6つ、`Stderr` の5つ、`Child` の11、`MutArray` を扱う7つ、
/// `Std.Par.par`、`Std.Async` の18行、`Std.Chan` の2行、`Std.Net` の12行以外は、引数も結果も `Unr` である。
#[test]
fn only_the_listed_rows_carry_a_lin_or_poly_mark() {
    use Mark::{Lin, Poly, Unr};
    for &e in Extern::ALL {
        let row = e.row();
        let expected: (&[Mark], Mark) = match row.name {
            "Std.FileIn.open" => (&[Unr], Lin),
            "Std.FileIn.read_all" => (&[Lin], Unr),
            "Std.FileIn.read_line_raw" => (&[Lin], Unr),
            "Std.FileIn.read_all_bytes" => (&[Lin], Unr),
            "Std.FileIn.read_line_bytes_raw" => (&[Lin], Unr),
            "Std.FileIn.read_chunk" => (&[Unr, Lin], Unr),
            "Std.FileIn.close" => (&[Lin], Unr),
            "Std.FileOut.create" | "Std.FileOut.append" => (&[Unr], Lin),
            "Std.FileOut.flush" => (&[Lin], Lin),
            "Std.FileOut.close_raw" => (&[Lin], Unr),
            "Std.Stdin.open" => (&[Unr], Lin),
            "Std.Stdin.read_all" => (&[Lin], Unr),
            "Std.Stdin.read_line_raw" => (&[Lin], Unr),
            "Std.Stdin.read_all_bytes" => (&[Lin], Unr),
            "Std.Stdin.read_line_bytes_raw" => (&[Lin], Unr),
            "Std.Stdin.read_chunk" => (&[Unr, Lin], Unr),
            "Std.Stdin.close" => (&[Lin], Unr),
            "Std.Stdout.open" | "Std.Stderr.open" => (&[Unr], Lin),
            "Std.Stdout.close" | "Std.Stderr.close" => (&[Lin], Unr),
            "Std.FileOut.write"
            | "Std.FileOut.write_bytes"
            | "Std.FileOut.write_line"
            | "Std.Stdout.write"
            | "Std.Stdout.write_bytes"
            | "Std.Stdout.write_line"
            | "Std.Stderr.write"
            | "Std.Stderr.write_bytes"
            | "Std.Stderr.write_line"
            | "Std.Proc.write"
            | "Std.Proc.write_bytes"
            | "Std.Proc.write_line" => (&[Unr, Lin], Lin),
            "Std.Proc.spawn_raw" => (&[Unr; 7], Lin),
            "Std.Proc.wait" => (&[Lin], Unr),
            "Std.Proc.close_stdin" => (&[Lin], Lin),
            "Std.Proc.read_all"
            | "Std.Proc.read_line_raw"
            | "Std.Proc.read_all_bytes"
            | "Std.Proc.read_line_bytes_raw" => (&[Lin], Unr),
            "Std.Proc.read_chunk" => (&[Unr, Lin], Unr),
            "Std.Stdout.flush" => (&[Lin], Lin),
            "Std.Array.thaw" => (&[Unr], Lin),
            "Std.MutArray.make" => (&[Unr, Unr], Lin),
            "Std.MutArray.empty" => (&[Unr], Lin),
            "Std.MutArray.length" => (&[Lin], Unr),
            "Std.MutArray.read" => (&[Lin, Unr], Unr),
            "Std.MutArray.write" => (&[Lin, Unr, Unr], Lin),
            "Std.MutArray.freeze" => (&[Lin], Unr),
            "Std.Par.par" => (&[Poly, Poly], Poly),
            "Std.Async.start_raw" | "Std.Async.run_raw" => (&[Poly], Lin),
            "Std.Async.park" => (&[Unr, Lin], Unr),
            "Std.Async.park_time" | "Std.Async.park_signal" => (&[Unr, Lin], Unr),
            "Std.Async.park_drain" => (&[Unr, Lin], Unr),
            "Std.Async.ready" => (&[Lin], Unr),
            "Std.Async.finish_ok" => (&[Poly], Unr),
            "Std.Async.task_cell" => (&[Lin], Unr),
            "Std.Async.take_result" | "Std.Async.run_result" => (&[Lin], Poly),
            "Std.Async.detach" | "Std.Async.cancel_raw" => (&[Lin], Unr),
            "Std.Async.start_in_raw" => (&[Unr, Poly], Lin),
            "Std.Chan.push_raw" => (&[Poly, Unr], Unr),
            "Std.Chan.pop_raw" => (&[Unr], Poly),
            "Std.Async.park_send" | "Std.Async.park_recv" => (&[Unr, Lin], Unr),
            "Std.Async.park_io" => (&[Unr, Lin], Unr),
            "Std.Async.park_io_until" => (&[Unr, Unr, Lin], Unr),
            "Std.Net.submit_accept" | "Std.Net.submit_read_line" => (&[Lin], Unr),
            "Std.Net.submit_read" | "Std.Net.submit_write" => (&[Unr, Lin], Unr),
            "Std.Net.op_listener" | "Std.Net.op_stream" => (&[Unr], Lin),
            "Std.Net.take_listener" | "Std.Net.take_stream" => (&[Unr], Lin),
            "Std.Net.local_port" | "Std.Net.peer_addr" => (&[Lin], Unr),
            "Std.Net.shutdown_write" => (&[Lin], Lin),
            "Std.Net.set_nodelay" => (&[Unr, Lin], Lin),
            _ => {
                assert!(
                    row.param_marks.iter().all(|&mark| mark == Unr),
                    "{}",
                    row.name
                );
                assert_eq!(row.ret_mark, Unr, "{}", row.name);
                continue;
            }
        };
        assert_eq!((row.param_marks, row.ret_mark), expected, "{}", row.name);
    }
}

/// 表の値そのもの。引数を預かる行は `Std.Async` の18行と `Std.Chan.push_raw` だけである。預かる引数は型変数を `Unr` に固定しない位置なので、
/// その印は `Unr` でない (docs/spec/declarations.md の「`extern`」)。
#[test]
fn only_the_listed_rows_hold_an_argument() {
    let holding: Vec<&str> = Extern::ALL
        .iter()
        .map(|e| e.row())
        .filter(|row| row.held.contains(&true))
        .map(|row| row.name)
        .collect();
    assert_eq!(
        holding,
        [
            "Std.Async.start_raw",
            "Std.Async.run_raw",
            "Std.Async.park",
            "Std.Async.park_time",
            "Std.Async.ready",
            "Std.Async.finish_ok",
            "Std.Async.task_cell",
            "Std.Async.take_result",
            "Std.Async.run_result",
            "Std.Async.detach",
            "Std.Async.cancel_raw",
            "Std.Async.start_in_raw",
            "Std.Async.park_drain",
            "Std.Async.park_send",
            "Std.Async.park_recv",
            "Std.Async.park_io",
            "Std.Async.park_io_until",
            "Std.Async.park_signal",
            "Std.Chan.push_raw",
        ]
    );
    for &e in Extern::ALL {
        let row = e.row();
        for (index, (&held, &mark)) in row.held.iter().zip(row.param_marks).enumerate() {
            assert!(
                !held || mark != Mark::Unr,
                "{} parameter {index} is held but `Unr`",
                row.name
            );
        }
    }
}

/// ランタイムが名前で呼ぶエントリは、std の単相の関数である。単相なので instance の名前が正式な名前のままになり、
/// 引数の Repr が宣言の型から決まる (docs/spec/stdlib.md の「`Std.Async`」)。
#[test]
fn every_runtime_entry_is_a_monomorphic_std_function() {
    let checked = checked();
    let program = &checked.program;
    let types = &checked.typed.types;
    for &entry in RuntimeEntry::ALL {
        let (id, function) = program
            .functions()
            .find(|(id, function)| {
                format!("{}.{}", program.modules[id.module].name, function.name) == entry.name()
            })
            .unwrap_or_else(|| panic!("the std defines `{}`", entry.name()));
        assert_eq!(function.kind, FunctionKind::Defined, "{}", entry.name());
        let signature = function.signature.as_ref().expect("a signature");
        assert!(
            signature.generics.type_vars.is_empty() && signature.generics.row_vars.is_empty(),
            "{} is generic",
            entry.name()
        );
        assert_eq!(program.arity(id), Some(entry.arity()), "{}", entry.name());
        let mut ty = checked.typed.decls[&ValueItem::Function(id)].ty;
        let mut params = Vec::new();
        for _ in 0..entry.arity() {
            let TypeKind::Fn { param, ret, .. } = types.kind(ty) else {
                panic!("`{}` has an arrow per parameter", entry.name());
            };
            params.push(boxed_repr(types, *param, program));
            ty = *ret;
        }
        assert_eq!(params, entry.params(), "{}", entry.name());
        assert_eq!(
            boxed_repr(types, ty, program),
            Repr::Unit,
            "{}",
            entry.name()
        );
    }
}

/// ランタイムが名前で呼ぶエントリは、下ろした後も宣言の名前のまま、型どおりの ABI で残る (docs/spec/core-ir.md の
/// 「変換の規則」)。
#[test]
fn the_runtime_entries_have_the_declared_abi() {
    let core = eml_test_support::core_until(
        "import Async\nmain : Unit -> Unit\nmain () = ()",
        eml_core_ir::Pass::Evidence,
    );
    for &entry in RuntimeEntry::ALL {
        let function = core
            .functions
            .iter()
            .find(|function| function.name == entry.name())
            .unwrap_or_else(|| panic!("`{}` is lowered", entry.name()));
        let params: Vec<Repr> = function
            .params()
            .iter()
            .map(|&param| function.repr(param))
            .collect();
        assert_eq!(params, entry.params(), "{}", entry.name());
        assert_eq!(function.split, None, "{}", entry.name());
        assert!(!function.internal, "{}", entry.name());
        assert_eq!(function.ret, Repr::Unit, "{}", entry.name());
    }
}

#[test]
fn every_extern_type_row_has_the_repr_of_its_type() {
    // 型検査は `Unit` を要素のないタプルにするので、`Prelude.Unit` の行は `types.rs` の要素のないタプルの規則と比べる
    let checked = checked();
    let types = &checked.typed.types;
    for &ty in ExternType::ALL {
        let row = ty.row();
        let name = probe(ty);
        let defined =
            |function: &Function| function.kind == FunctionKind::Defined && function.name == name;
        let TypeKind::Fn { param, .. } = types.kind(declared(&checked, defined)) else {
            panic!("`{name}` is a function");
        };
        assert_eq!(
            boxed_repr(types, *param, &checked.program),
            row.repr,
            "{}",
            row.name
        );
    }
}

/// 機械の extern は `Bool` の値を、translate は `with_arg` の行の組を、配置の表を見ずに `FALSE`、`TRUE`、`TUPLE` の
/// タグで作る。そのタグが、translate が std の宣言から作る配置の添字と合うことを確かめる。
#[test]
fn the_tags_externs_build_name_the_constructors_of_their_layouts() {
    let program = eml_test_support::lir(
        "import FileIn\nmain : Unit -> <IO> Unit\nmain () =\n  let f = FileIn.open \"a.txt\"\n  let (text, f) = FileIn.read_all f\n  FileIn.close f\n  if text == \"\" then println \"empty\" else println text",
    );
    let constructors = |name: &str| -> Vec<&str> {
        let layout = program
            .layouts
            .iter()
            .find(|layout| layout.name == name)
            .unwrap_or_else(|| panic!("no layout `{name}`"));
        layout
            .constructors
            .iter()
            .map(|ctor| ctor.name.as_str())
            .collect()
    };
    let bool = constructors("Prelude.Bool");
    assert_eq!(bool, ["False", "True"]);
    assert_eq!(
        (bool[FALSE as usize], bool[TRUE as usize]),
        ("False", "True")
    );
    // `FileIn.read_all` の結果と `FileIn` の組は、要素が2つの組の配置の唯一のコンストラクタである
    assert_eq!(constructors("(,)"), ["(,)"]);
    assert_eq!(TUPLE, 0);
}

/// `compare` の extern は、配置の表を見ずに `LT`、`EQ`、`GT` のタグで値を作る。そのタグが Prelude の `Ordering` の
/// 宣言の順と一致することを確かめる。
#[test]
fn the_ordering_tags_match_the_prelude_declaration() {
    let lowered = eml_test_support::lower("");
    let program = &lowered.program;
    let ordering = program.lang.ordering;
    let eml_hir::TypeDefKind::Data { constructors } = &program[ordering].kind else {
        panic!("`Ordering` is a data type");
    };
    let names: Vec<(&str, u32)> = constructors
        .iter()
        .map(|&c| (program[c].name.as_str(), program[c].tag))
        .collect();
    assert_eq!(
        names,
        [
            ("LT", eml_lir::LT),
            ("EQ", eml_lir::EQ),
            ("GT", eml_lir::GT)
        ]
    );
}

/// VM は `try_io` の `io_failed` の節に `IOErrorKind` のタグを渡す。そのタグが Prelude の宣言の順と一致することを
/// 確かめる (docs/spec/stdlib.md の「`try_io`」)。
#[test]
fn the_io_error_kind_tags_match_the_prelude_declaration() {
    let lowered = eml_test_support::lower("");
    let program = &lowered.program;
    let (_, kind) = program
        .types()
        .find(|(id, def)| program.modules[id.module].name == "Prelude" && def.name == "IOErrorKind")
        .expect("the Prelude declares `IOErrorKind`");
    let eml_hir::TypeDefKind::Data { constructors } = &kind.kind else {
        panic!("`IOErrorKind` is a data type");
    };
    let names: Vec<(&str, u32)> = constructors
        .iter()
        .map(|&c| (program[c].name.as_str(), program[c].tag))
        .collect();
    assert_eq!(
        names,
        [
            ("NotFound", eml_lir::IO_NOT_FOUND),
            ("PermissionDenied", eml_lir::IO_PERMISSION_DENIED),
            ("NotAvailable", eml_lir::IO_NOT_AVAILABLE),
            ("NotUtf8", eml_lir::IO_NOT_UTF8),
            ("Other", eml_lir::IO_OTHER),
            ("Cancelled", eml_lir::IO_CANCELLED),
            // 次の5つには Rust の定数がない。Rust の側は作らず、`fail_io` がタグをそのまま渡す
            ("TimedOut", 6),
            ("ConnectionReset", 7),
            ("AddrInUse", 8),
            ("ResourceLimit", 9),
            ("ConnectionRefused", 10)
        ]
    );
}

/// ランタイムは待たせたタスクを再開するときに、配置の表を見ずに `WAKE_WOKE` と `WAKE_CANCELLED` のタグで `Wake` を
/// 作る。そのタグが `Std.Async` の宣言の順と一致することを確かめる (docs/spec/stdlib.md の「`Std.Async`」)。
#[test]
fn the_wake_tags_match_the_async_declaration() {
    let lowered = eml_test_support::lower("import Async\n");
    let program = &lowered.program;
    let (_, wake) = program
        .types()
        .find(|(id, def)| program.modules[id.module].name == "Std.Async" && def.name == "Wake")
        .expect("`Std.Async` declares `Wake`");
    let eml_hir::TypeDefKind::Data { constructors } = &wake.kind else {
        panic!("`Wake` is a data type");
    };
    let names: Vec<(&str, u32)> = constructors
        .iter()
        .map(|&c| (program[c].name.as_str(), program[c].tag))
        .collect();
    assert_eq!(
        names,
        [
            ("Woke", eml_lir::WAKE_WOKE),
            ("Cancelled", eml_lir::WAKE_CANCELLED)
        ]
    );
}
