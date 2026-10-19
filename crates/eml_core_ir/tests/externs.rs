//! extern の表の行の Repr と、std の宣言の型の Repr の照らし合わせ (docs/spec/core-ir.md)。行の名前と矢印の数は
//! `eml_hir` の結合テストが照らし合わせる。型から Repr を決める規則は translate にあるので、ここで確かめる。
//! extern が作る値のタグと、translate が作る配置も照らし合わせる。

use eml_core_ir::type_repr;
use eml_extern::{Extern, ExternType, Linearity, OpRet, Purity};
use eml_hir::{Function, FunctionKind, MethodImpl, TypeDefId, ValueItem};
use eml_lir::{FALSE, TRUE, TUPLE};
use eml_test_support::Checked;
use eml_types::{Lin, Substitution, TypeId, TypeKind, TypeStore};

/// 型の行ごとに、その型の値をそのまま返す関数を置く。型検査がその型に与える型を、関数の引数の型から読む。標準
/// ライブラリは使うモジュールだけを読み込むので、すべてのモジュールを import する (`probes`)。
const PROBES: &str = "\
probe_int : Int -> Int
probe_int x = x

probe_string : String -> String
probe_string s = s

probe_float : Float -> Float
probe_float x = x

probe_unit : Unit -> Unit
probe_unit u = u

probe_file : Fs.File -> Fs.File
probe_file f = f

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
        ExternType::String => "probe_string",
        ExternType::Unit => "probe_unit",
        ExternType::File => "probe_file",
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

fn has_type_var(types: &TypeStore, ty: TypeId) -> bool {
    match types.kind(ty) {
        TypeKind::Con {
            id: _,
            args,
            rows: _,
        } => args.iter().any(|&arg| has_type_var(types, arg)),
        TypeKind::Tuple(elements) => elements.iter().any(|&ty| has_type_var(types, ty)),
        TypeKind::Fn {
            param,
            lin: _,
            effects: _,
            tail: _,
            ret,
        } => has_type_var(types, *param) || has_type_var(types, *ret),
        TypeKind::Rigid(_) | TypeKind::OpVar(_, _) | TypeKind::Flexible(_) => true,
        TypeKind::Error => false,
    }
}

#[test]
fn every_extern_function_row_has_the_reprs_of_its_std_signature() {
    // 行の Repr は引数の型と結果の型から比べる。型変数を持てるのは、戻らない extern の結果だけである
    // (docs/spec/lir.md の「戻らない extern」)
    let checked = checked();
    let mut store = checked.typed.types.clone();
    for &e in Extern::ALL {
        let row = e.row();
        let mut ty = row_type(&checked, &mut store, e);
        let types = &store;
        let mut params = Vec::new();
        for _ in row.params {
            let TypeKind::Fn { param, ret, .. } = types.kind(ty) else {
                panic!("`{}` has an arrow per parameter", row.name);
            };
            assert!(!has_type_var(types, *param), "{}", row.name);
            params.push(type_repr(types, *param, &checked.program));
            ty = *ret;
        }
        assert_eq!(params, row.params, "{}", row.name);
        match row.ret {
            OpRet::Repr(ret) => {
                assert!(!has_type_var(types, ty), "{}", row.name);
                assert_eq!(type_repr(types, ty, &checked.program), ret, "{}", row.name);
            }
            OpRet::Never => assert!(
                has_type_var(types, ty),
                "`{}` never returns, so its result is a type variable",
                row.name
            ),
        }
    }
}

#[test]
fn faulting_rows_carry_panic() {
    // 行の `purity` は、std の型の最後の外側の矢印の row と合う。`Panics` は `Panic` を起こし `IO` を起こさない。
    // `Effectful` は `IO` を起こし、`Std.Env.arg` のように `Panic` も起こしうる。`Pure` は何も起こさない
    // (docs/spec/effects.md の「組み込みの `Panic`」)
    let checked = checked();
    let program = &checked.program;
    let (io, panic) = (program.io(), program.panic());
    let mut store = checked.typed.types.clone();
    for &e in Extern::ALL {
        let row = e.row();
        let mut ty = row_type(&checked, &mut store, e);
        let mut labels = Vec::new();
        for _ in row.params {
            let TypeKind::Fn {
                effects, tail, ret, ..
            } = store.kind(ty)
            else {
                panic!("`{}` has an arrow per parameter", row.name);
            };
            assert_eq!(*tail, None, "{}", row.name);
            labels = effects.iter().map(|label| label.id).collect();
            ty = *ret;
        }
        match row.purity {
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

#[test]
fn every_extern_function_row_has_the_kinds_of_its_std_signature() {
    let checked = checked();
    let mut store = checked.typed.types.clone();
    for &e in Extern::ALL {
        let row = e.row();
        let mut ty = row_type(&checked, &mut store, e);
        let types = &store;
        assert_eq!(row.param_kinds.len(), row.params.len(), "{}", row.name);
        let mut params = Vec::new();
        for _ in row.params {
            let TypeKind::Fn { param, ret, .. } = types.kind(ty) else {
                panic!("`{}` has an arrow per parameter", row.name);
            };
            params.push(kind_of(types.linearity(*param)));
            ty = *ret;
        }
        assert_eq!(params, row.param_kinds, "{}", row.name);
        if let OpRet::Repr(_) = row.ret {
            assert_eq!(kind_of(types.linearity(ty)), row.ret_kind, "{}", row.name);
        }
    }
}

fn kind_of(lin: Lin) -> Linearity {
    match lin {
        Lin::Unr => Linearity::Unr,
        Lin::Lin => Linearity::Lin,
        Lin::Poly | Lin::Var(_) => panic!("an extern row has no polymorphic kind"),
    }
}

/// 表の値そのもの。`Fs` の4つ以外は、引数も結果も `Unr` である。
#[test]
fn only_the_file_functions_carry_a_lin() {
    use Linearity::{Lin, Unr};
    for &e in Extern::ALL {
        let row = e.row();
        let expected: (&[Linearity], Linearity) = match row.name {
            "Std.Fs.open" => (&[Unr], Lin),
            "Std.Fs.read_all" => (&[Lin], Lin),
            "Std.Fs.read_line_raw" => (&[Lin], Lin),
            "Std.Fs.close" => (&[Lin], Unr),
            _ => {
                assert!(
                    row.param_kinds.iter().all(|&kind| kind == Unr),
                    "{}",
                    row.name
                );
                assert_eq!(row.ret_kind, Unr, "{}", row.name);
                continue;
            }
        };
        assert_eq!((row.param_kinds, row.ret_kind), expected, "{}", row.name);
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
            type_repr(types, *param, &checked.program),
            row.repr,
            "{}",
            row.name
        );
    }
}

/// 機械の extern は、`Bool` と組の値を配置の表を見ずに `FALSE`、`TRUE`、`TUPLE` のタグで作る。そのタグが、translate
/// が std の宣言から作る配置の添字と合うことを確かめる。
#[test]
fn the_tags_externs_build_name_the_constructors_of_their_layouts() {
    let program = eml_test_support::lir(
        "main : Unit -> <IO> Unit\nmain () =\n  let f = Fs.open \"a.txt\"\n  let (f, text) = Fs.read_all f\n  Fs.close f\n  if text == \"\" then println \"empty\" else println text",
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
    // `Fs.read_all` の結果は、要素が2つの組の配置の唯一のコンストラクタである
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
            ("Other", eml_lir::IO_OTHER)
        ]
    );
}
