//! Core IR のパスの順番 (docs/spec/core-ir.md)。順番を知っているのはこのファイルだけにする。

use eml_hir::Module;
use eml_types::TypedModule;

use crate::{Program, perceus, simplify, translate};

/// 診断のエラーがないプログラムだけを受け取る。エラーがあれば `eml_cli` は Core IR を作らない
/// (docs/implementation/architecture.md)。
pub fn lower(module: &Module, typed: &TypedModule) -> Program {
    let mut program = translate::translate(module, typed);
    simplify::simplify(&mut program);
    perceus::insert(&mut program);
    // Perceus の誤りを、実行した経路だけでなく変換のたびに見つける (docs/spec/core-ir.md)
    #[cfg(debug_assertions)]
    if let Err(error) = crate::verify(&program) {
        panic!("internal error: invalid Core IR: {error}");
    }
    program
}
