//! 型、row、Kind の検査 (docs/spec/types.md)。

mod carry;
mod check;
mod context;
mod data;
mod exhaustive;
mod kind;
mod scc;
mod shape;
mod table;
mod ty;
mod usage;

use std::fmt::Write;

use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};
use eml_hir::{Constructor, ExprId, Function, ItemMap, LocalId, Operation, PatId, Program};
use la_arena::ArenaMap;

pub use ty::{
    ContState, EffectLabel, KindConstraint, KindTerm, Linearity, Multiplicity, RowTail, RowTerm,
    Type,
};

pub mod codes {
    use eml_diagnostics::ErrorCode;

    pub const LINEAR_VALUE_MISUSED: ErrorCode = ErrorCode(3001);
    pub const LINEAR_VALUE_USED_TWICE: ErrorCode = ErrorCode(3002);
    pub const LINEAR_VALUE_NOT_CONSUMED: ErrorCode = ErrorCode(3003);
    pub const LINEAR_VALUE_DISCARDED: ErrorCode = ErrorCode(3004);
    pub const CONTINUATION_NOT_HANDLED: ErrorCode = ErrorCode(3005);
    pub const LINEAR_VALUE_KEPT_ACROSS_MULTI: ErrorCode = ErrorCode(3006);
    pub const TYPE_MISMATCH: ErrorCode = ErrorCode(2001);
    pub const EFFECT_NOT_IN_ROW: ErrorCode = ErrorCode(2002);
    pub const MISSING_MAIN: ErrorCode = ErrorCode(2003);
    pub const INVALID_MAIN_TYPE: ErrorCode = ErrorCode(2004);
    pub const INFINITE_TYPE: ErrorCode = ErrorCode(2005);
    pub const NOT_COMPARABLE: ErrorCode = ErrorCode(2006);
    pub const RESUME_STATE_MISMATCH: ErrorCode = ErrorCode(2007);
    pub const NON_EXHAUSTIVE_MATCH: ErrorCode = ErrorCode(4001);
    pub const NON_EXHAUSTIVE_EQUATION: ErrorCode = ErrorCode(4002);
    pub const REFUTABLE_PATTERN: ErrorCode = ErrorCode(4003);
    /// 重大度は Warning である (docs/spec/diagnostics.md の「網羅性の診断」)。
    pub const UNREACHABLE_ARM: ErrorCode = ErrorCode(4004);
    /// 重大度は Warning である (docs/spec/diagnostics.md の「網羅性の診断」)。
    pub const UNREACHABLE_EQUATION: ErrorCode = ErrorCode(4005);
}

/// 型付き HIR。HIR は複製せず、型を別テーブルに持つ (docs/implementation/architecture.md)。
#[derive(Debug, Default)]
pub struct TypedModule {
    /// シグネチャのある関数だけを含む。
    pub signatures: ItemMap<Function, Scheme>,
    /// シグネチャと等式の両方がある関数だけを含む。
    pub bodies: ItemMap<Function, BodyTypes>,
    /// エフェクトの操作のスキーム。Core IR が、操作を包む関数の変数を boxed にするかを決めるのに使う。
    pub operations: ItemMap<Operation, Scheme>,
    /// コンストラクタのスキーム。`Some : a -> Option a` の形である。Core IR が、コンストラクタを包む関数の変数を
    /// boxed にするかを決めるのに使う。
    pub constructors: ItemMap<Constructor, Scheme>,
}

/// 関数の型と、多相化したときに残った Kind の制約のうち、定数を片側に持つもの。変数どうしの制約は部分適用のたびに
/// 増えて読みにくくなるので出さない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scheme {
    pub ty: Type,
    pub constraints: Vec<KindConstraint>,
}

/// `==` と `!=` の比べ方。型クラスがないので、型検査が引数の型から決め、比べられる型を限る
/// (docs/spec/declarations.md の標準の演算子の表)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Equality {
    Int,
    String,
    Bool,
}

#[derive(Debug, Default)]
pub struct BodyTypes {
    pub exprs: ArenaMap<ExprId, Type>,
    pub locals: ArenaMap<LocalId, Type>,
    /// パターンが受けた値の型。Core IR が、handler の節の引数の変数を作るのに使う。
    pub pats: ArenaMap<PatId, Type>,
    /// `==` と `!=` の比べ方。キーは演算子を指す呼ばれる側の式である。Core IR が、どの比べる命令にするかを決めるのに
    /// 使う。
    pub equalities: ArenaMap<ExprId, Equality>,
}

pub fn check(program: &Program) -> (TypedModule, Vec<Diagnostic>) {
    check::check_module(program)
}

/// `main` がないこと。`eml check` では検査せず、`eml run` だけが報告する (docs/spec/types.md の「推論」)。
pub fn missing_main(file: FileId) -> Diagnostic {
    Diagnostic::error(
        codes::MISSING_MAIN,
        "`main` is not defined",
        Label::new(
            file,
            TextRange::empty(0.into()),
            "`eml run` starts the program from `main`",
        ),
    )
    .with_help("add `main : Unit -> <IO> Unit` and an equation `main () = ...`")
}

/// テストで推論結果を確かめるための表示。入口のモジュールだけを表示する。Prelude はどのプログラムにもあるので、
/// テストの表示を Prelude に左右させないため。
pub fn dump(program: &Program, typed: &TypedModule) -> String {
    let mut out = String::new();
    for (id, operation) in program.operations() {
        if id.module != program.entry {
            continue;
        }
        if let Some(scheme) = typed.operations.get(id) {
            writeln!(out, "{} : {}", operation.name, scheme.ty).unwrap();
            write_kinds(&mut out, scheme);
        }
    }
    for (id, function) in program.functions() {
        if id.module != program.entry {
            continue;
        }
        if let Some(scheme) = typed.signatures.get(id) {
            writeln!(out, "{} : {}", function.name, scheme.ty).unwrap();
            write_kinds(&mut out, scheme);
        }
        let (Some(body), Some(types)) = (program.body(id), typed.bodies.get(id)) else {
            continue;
        };
        for (local, data) in body.locals.iter() {
            if let Some(ty) = types.locals.get(local) {
                writeln!(
                    out,
                    "  {}#{} : {ty}",
                    data.name,
                    u32::from(local.into_raw())
                )
                .unwrap();
            }
        }
    }
    out
}

fn write_kinds(out: &mut String, scheme: &Scheme) {
    if !scheme.constraints.is_empty() {
        let kinds: Vec<String> = scheme.constraints.iter().map(ToString::to_string).collect();
        writeln!(out, "  kinds: {}", kinds.join(", ")).unwrap();
    }
}

/// 単体テストのための `Program`。Prelude と、`text` を入口にしたモジュールを変換する。
#[cfg(test)]
pub(crate) fn test_program(text: &str) -> eml_hir::Program {
    let mut files = eml_diagnostics::SourceFiles::new();
    let prelude = files.add(eml_hir::PRELUDE_PATH, eml_hir::PRELUDE_SOURCE);
    let main = files.add("test.em", text);
    let (parse, _) = eml_syntax::parse(main, files.text(main));
    let prelude_tree = eml_hir::parse_prelude(prelude);
    let trees = [
        eml_hir::item_tree(prelude, &prelude_tree).0,
        eml_hir::item_tree(main, &parse.tree()).0,
    ];
    let (def_map, _) = eml_hir::def_map(&trees);
    eml_hir::lower(&def_map, &trees).0
}
