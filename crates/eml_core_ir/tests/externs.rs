//! extern の表の行の Repr と、std の宣言の型の Repr の照らし合わせ (docs/spec/core-ir.md)。行の名前と矢印の数は
//! `eml_hir` の結合テストが照らし合わせる。型から Repr を決める規則は translate にあるので、ここで確かめる。
//! extern が作る値のタグと、translate が作る配置も照らし合わせる。

use eml_core_ir::{FALSE, TRUE, TUPLE, type_repr};
use eml_extern::{Extern, ExternType};
use eml_hir::{Function, FunctionKind, MethodImpl, TypeDefId, ValueItem};
use eml_test_support::Checked;
use eml_types::{Substitution, TypeId, TypeKind, TypeStore};

/// 型の行ごとに、その型の値をそのまま返す関数を置く。型検査がその型に与える型を、関数の引数の型から読む。
const PROBES: &str = "\
import Std.Fs

probe_int : Int -> Int
probe_int x = x

probe_string : String -> String
probe_string s = s

probe_unit : Unit -> Unit
probe_unit u = u

probe_file : Fs.File -> Fs.File
probe_file f = f

main : Unit -> <IO> Unit
main () = println \"x\"
";

/// 型の行を読む `PROBES` の関数。`_` の腕を書かないので、型の行を足すとここがコンパイルできなくなる。
fn probe(ty: ExternType) -> &'static str {
    match ty {
        ExternType::Int => "probe_int",
        ExternType::String => "probe_string",
        ExternType::Unit => "probe_unit",
        ExternType::File => "probe_file",
    }
}

fn checked() -> Checked {
    let checked = eml_test_support::check(PROBES);
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
    types.substitute(declared, &mut Substitution::new([(var, head)]))
}

/// 型引数のない型 `head` を指す、型の表の型。テストは型の表に型を作れないので、宣言の型の中から探す。
fn head_type(checked: &Checked, head: TypeDefId) -> TypeId {
    let types = &checked.typed.types;
    let mut pending: Vec<TypeId> = checked.typed.decls.values().map(|decl| decl.ty).collect();
    while let Some(ty) = pending.pop() {
        match types.kind(ty) {
            TypeKind::Con { id, args } if *id == head && args.is_empty() => return ty,
            TypeKind::Con { id: _, args } => pending.extend(args),
            TypeKind::Fn { param, ret, .. } => pending.extend([*param, *ret]),
            TypeKind::Record(fields) => pending.extend(fields.iter().map(|&(_, ty)| ty)),
            TypeKind::Rigid(_) | TypeKind::OpVar(_) | TypeKind::Flexible | TypeKind::Error => {}
        }
    }
    panic!(
        "`{}` appears in a declared type",
        checked.program[head].name
    )
}

fn has_type_var(types: &TypeStore, ty: TypeId) -> bool {
    match types.kind(ty) {
        TypeKind::Con { id: _, args } => args.iter().any(|&arg| has_type_var(types, arg)),
        TypeKind::Record(fields) => fields.iter().any(|&(_, ty)| has_type_var(types, ty)),
        TypeKind::Fn {
            param,
            effects: _,
            tail: _,
            ret,
        } => has_type_var(types, *param) || has_type_var(types, *ret),
        TypeKind::Rigid(_) | TypeKind::OpVar(_) | TypeKind::Flexible => true,
        TypeKind::Error => false,
    }
}

#[test]
fn every_extern_function_row_has_the_reprs_of_its_std_signature() {
    // 行の Repr は型変数の位置を `tobj` として比べる。多相な extern は S12 で入り、そこで比べ方を決め直す
    // (docs/future/roadmap.md)
    let checked = checked();
    let mut store = checked.typed.types.clone();
    for &e in Extern::ALL {
        let row = e.row();
        let mut ty = row_type(&checked, &mut store, e);
        let types = &store;
        assert!(!has_type_var(types, ty), "{}", row.name);
        let mut params = Vec::new();
        for _ in row.params {
            let TypeKind::Fn { param, ret, .. } = types.kind(ty) else {
                panic!("`{}` has an arrow per parameter", row.name);
            };
            params.push(type_repr(types, *param, &checked.program));
            ty = *ret;
        }
        assert_eq!(params, row.params, "{}", row.name);
        assert_eq!(
            type_repr(types, ty, &checked.program),
            row.ret,
            "{}",
            row.name
        );
    }
}

#[test]
fn every_extern_type_row_has_the_repr_of_its_type() {
    // 型検査は `Unit` を空のレコードにするので、`Prelude.Unit` の行は `types.rs` の空のレコードの規則と比べる
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
    let program = eml_test_support::core(
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
            ("LT", eml_core_ir::LT),
            ("EQ", eml_core_ir::EQ),
            ("GT", eml_core_ir::GT)
        ]
    );
}
