//! CST から HIR への変換と名前解決 (docs/implementation/architecture.md の「`eml_hir` で行う脱糖と検査」)。

mod def_map;
mod eval;
mod hir;
mod item_tree;
mod load;
mod lower;
mod names;
mod pretty;
mod program;

pub use def_map::*;
pub use eval::{EvalStep, call_steps, is_value, known_arity};
pub use hir::*;
pub use item_tree::*;
pub use load::*;
pub use lower::lower;
pub use names::DisplayNames;
pub use pretty::pretty;
pub use program::*;

/// 埋め込んだ標準ライブラリ。リポジトリの根の `std/` のファイル名と本文の組で、最初が Prelude である。`load` が読む。
/// crate の外のファイルを埋め込むので `cargo package` は通らないが、eml は公開前なので受け入れた。並びが `std/` の
/// ファイルと一致することは `eml_hir` の結合テストが確かめる。
pub const STD: &[(&str, &str)] = &[
    ("Prelude.em", include_str!("../../../std/Prelude.em")),
    ("Fs.em", include_str!("../../../std/Fs.em")),
];

/// Prelude のソース。標準ライブラリの Prelude を差し替えるテストが、本文に宣言を足すのに使う。
pub const PRELUDE_SOURCE: &str = STD[0].1;

/// 診断の表示に使う Prelude のパス。標準ライブラリのパスは `<std>/` で始め、手元の相対パスと見誤らないようにする。
pub const PRELUDE_PATH: &str = "<std>/Prelude.em";

pub mod codes {
    use eml_diagnostics::ErrorCode;

    pub const UNDEFINED_NAME: ErrorCode = ErrorCode(1001);
    pub const UNDEFINED_TYPE: ErrorCode = ErrorCode(1002);
    pub const DUPLICATE_DEFINITION: ErrorCode = ErrorCode(1003);
    pub const MISSING_SIGNATURE: ErrorCode = ErrorCode(1004);
    pub const MISSING_EQUATION: ErrorCode = ErrorCode(1005);
    pub const NON_ASSOCIATIVE_OPERATORS: ErrorCode = ErrorCode(1006);
    pub const INVALID_OPERATION_SIGNATURE: ErrorCode = ErrorCode(1007);
    pub const NEVER_RESULT_NOT_FREE: ErrorCode = ErrorCode(1008);
    pub const UNHANDLEABLE_EFFECT: ErrorCode = ErrorCode(1009);
    pub const CLAUSE_ARITY: ErrorCode = ErrorCode(1010);
    pub const DROP_ARITY: ErrorCode = ErrorCode(1011);
    pub const MIXED_EFFECTS_IN_HANDLER: ErrorCode = ErrorCode(1012);
    pub const MISSING_CLAUSE: ErrorCode = ErrorCode(1013);
    pub const DUPLICATE_CLAUSE: ErrorCode = ErrorCode(1014);
    pub const TYPE_ARGUMENT_COUNT: ErrorCode = ErrorCode(1015);
    pub const CONSTRUCTOR_ARITY: ErrorCode = ErrorCode(1016);
    pub const DUPLICATE_BINDING: ErrorCode = ErrorCode(1017);
    pub const NON_CONSECUTIVE_EQUATIONS: ErrorCode = ErrorCode(1018);
    pub const SIGNATURE_NOT_ADJACENT: ErrorCode = ErrorCode(1019);
    pub const EQUATION_ARITY_MISMATCH: ErrorCode = ErrorCode(1020);
    pub const DUPLICATE_FIXITY: ErrorCode = ErrorCode(1021);
    pub const FIXITY_WITHOUT_DEFINITION: ErrorCode = ErrorCode(1022);
    pub const INVALID_SECTION: ErrorCode = ErrorCode(1023);
    pub const USE_AT_END_OF_BLOCK: ErrorCode = ErrorCode(1024);
    pub const MISSING_CONSTRUCTORS: ErrorCode = ErrorCode(1025);
    pub const MODULE_NOT_FOUND: ErrorCode = ErrorCode(1026);
    pub const IMPORT_CYCLE: ErrorCode = ErrorCode(1027);
    pub const AMBIGUOUS_NAME: ErrorCode = ErrorCode(1028);
    pub const PRIVATE_NAME: ErrorCode = ErrorCode(1029);
    pub const RESERVED_MODULE: ErrorCode = ErrorCode(1030);
    pub const UNKNOWN_QUALIFIER: ErrorCode = ErrorCode(1031);
    pub const PRIVATE_IN_PUBLIC: ErrorCode = ErrorCode(1032);
    pub const EXTERN_OUTSIDE_STD: ErrorCode = ErrorCode(1033);
}
