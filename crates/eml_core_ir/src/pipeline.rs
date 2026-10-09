//! Core IR のパスの順番 (docs/spec/core-ir.md の「パス」)。順番を知っているのはこのファイルだけにする。テストは
//! `lower_until` で、確かめたいパスの直後の IR を見る。

use eml_diagnostics::SourceFiles;
use eml_hir::{FunctionId, Program as HirProgram};
use eml_types::TypedProgram;

use crate::{
    Program, VerifyError, boxing, contract, perceus, translate, verify, verify_scopes,
    verify_translated,
};

/// `lower_until` で止める位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pass {
    Translate,
    Boxing,
    Contract,
    Perceus,
}

impl Pass {
    fn name(self) -> &'static str {
        match self {
            Pass::Translate => "translate",
            Pass::Boxing => "boxing",
            Pass::Contract => "contract",
            Pass::Perceus => "perceus",
        }
    }
}

/// 診断のエラーがないプログラムだけを受け取る。エラーがあれば `eml_cli` は Core IR を作らない
/// (docs/implementation/architecture.md)。`files` は、extern の呼び出しの位置 (`Loc`) を行と列にするのに使う。
///
/// `entry` は本体を持つ関数 (extern ではない) で、型が `Unit -> ...` か、引数がなく `Unit -> ...` の関数を返すものでなければならない。
/// `main` は型検査器が E2004 で保証するが、ほかの呼び出し元 (将来の REPL など) は自分で保証する。
pub fn lower(
    hir: &HirProgram,
    typed: &TypedProgram,
    entry: FunctionId,
    files: &SourceFiles,
) -> Program {
    lower_until(hir, typed, entry, files, Pass::Perceus)
}

/// `last` の直後で止める。止めたパスまでの検査は、debug ビルドでかける。
pub fn lower_until(
    hir: &HirProgram,
    typed: &TypedProgram,
    entry: FunctionId,
    files: &SourceFiles,
    last: Pass,
) -> Program {
    let mut program = translate::translate(hir, typed, entry, files);
    check(&program, Pass::Translate);
    if last == Pass::Translate {
        return program;
    }
    boxing(&mut program);
    check(&program, Pass::Boxing);
    if last == Pass::Boxing {
        return program;
    }
    contract(&mut program);
    check(&program, Pass::Contract);
    if last == Pass::Contract {
        return program;
    }
    perceus(&mut program);
    check(&program, Pass::Perceus);
    program
}

/// 誤りを、それを作ったパスの名前で報告する。実行した経路だけでなく、変換のたびに見つけるためである。
fn check(program: &Program, pass: Pass) {
    if !cfg!(debug_assertions) {
        return;
    }
    let result = match pass {
        Pass::Translate => verify_translated(program),
        Pass::Boxing | Pass::Contract => verify_scopes(program),
        Pass::Perceus => verify(program),
    };
    if let Err(error) = result {
        fail(pass, error);
    }
}

fn fail(pass: Pass, error: VerifyError) -> ! {
    panic!(
        "internal error: invalid Core IR after {}: {error}",
        pass.name()
    );
}
