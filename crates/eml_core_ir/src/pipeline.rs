//! Core IR のパスの順番 (docs/spec/core-ir.md)。順番を知っているのはこのファイルだけにする。テストは `lower_until`
//! で、確かめたいパスの直後の IR を見る。

use eml_hir::Module;
use eml_types::TypedModule;

use crate::{Program, liveness, perceus, simplify, translate, verify, verify_scopes};

/// `lower_until` で止める位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pass {
    Translate,
    Simplify,
    Perceus,
}

impl Pass {
    fn name(self) -> &'static str {
        match self {
            Pass::Translate => "translate",
            Pass::Simplify => "simplify",
            Pass::Perceus => "perceus",
        }
    }
}

/// 診断のエラーがないプログラムだけを受け取る。エラーがあれば `eml_cli` は Core IR を作らない
/// (docs/implementation/architecture.md)。
pub fn lower(module: &Module, typed: &TypedModule) -> Program {
    lower_until(module, typed, Pass::Perceus)
}

/// `last` の直後で止める。止めたパスまでの検査は、debug ビルドでかける。
pub fn lower_until(module: &Module, typed: &TypedModule, last: Pass) -> Program {
    let mut program = translate::translate(module, typed);
    settle(&mut program, Pass::Translate);
    if last == Pass::Translate {
        return program;
    }
    simplify::simplify(&mut program);
    settle(&mut program, Pass::Simplify);
    if last == Pass::Simplify {
        return program;
    }
    perceus::insert(&mut program);
    check(&program, Pass::Perceus);
    program
}

/// パスの中では `captures` が古くなってよいが、パスの間ではつねに正しくする (docs/spec/core-ir.md のパスの表)。
/// 途中で止めた IR の表示と、`verify_scopes` が `captures` を宣言として扱うためである。
fn settle(program: &mut Program, pass: Pass) {
    for function in &mut program.functions {
        liveness::analyze(function);
    }
    check(program, pass);
}

/// 誤りを、それを作ったパスの名前で報告する。実行した経路だけでなく、変換のたびに見つけるためである。
fn check(program: &Program, pass: Pass) {
    if !cfg!(debug_assertions) {
        return;
    }
    let result = match pass {
        Pass::Translate | Pass::Simplify => verify_scopes(program),
        Pass::Perceus => verify(program),
    };
    if let Err(error) = result {
        panic!(
            "internal error: invalid Core IR after {}: {error}",
            pass.name()
        );
    }
}
