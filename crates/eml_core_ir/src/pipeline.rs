//! Core IR のパスの順番と、LIR へのつなぎ (docs/spec/core-ir.md の「パス」)。2つの層をつなぐ順番を知っているのは
//! このファイルだけにする。

use eml_diagnostics::SourceFiles;
use eml_hir::{FunctionId, Program as HirProgram};
use eml_types::TypedProgram;

use crate::{Program, evidence, optimize, to_lir, translate, verify_evidence, verify_translated};
use eml_lir::VerifyError;

/// `core_until` で止める位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pass {
    Translate,
    Optimize,
    Evidence,
}

impl Pass {
    fn name(self) -> &'static str {
        match self {
            Pass::Translate => "translate",
            Pass::Optimize => "optimize",
            Pass::Evidence => "evidence",
        }
    }
}

/// `last` の直後の Core IR で止める。止めたパスまでの検査は、debug ビルドでかける。
///
/// 診断のエラーがないプログラムだけを受け取る。エラーがあれば `eml_cli` は Core IR を作らない
/// (docs/implementation/architecture.md)。`files` は、extern の呼び出しの位置 (`Loc`) を行と列にするのに使う。
///
/// `entry` は本体を持つ関数 (extern ではない) で、型が `Unit -> ...` か、引数がなく `Unit -> ...` の関数を返すものでなければならない。
/// `main` は型検査器が E2004 で保証するが、ほかの呼び出し元 (将来の REPL など) は自分で保証する。
pub fn core_until(
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
    optimize(&mut program);
    check(&program, Pass::Optimize);
    if last == Pass::Optimize {
        return program;
    }
    evidence(&mut program);
    check(&program, Pass::Evidence);
    program
}

/// 入力の条件は `core_until` と同じである。
pub fn lower(
    hir: &HirProgram,
    typed: &TypedProgram,
    entry: FunctionId,
    files: &SourceFiles,
) -> eml_lir::Program {
    lower_until(hir, typed, entry, files, eml_lir::Pass::Reuse)
}

/// EP の後の Core IR を LIR に写し、`last` までの LIR のパスをかける。入力の条件は `core_until` と同じである。
pub fn lower_until(
    hir: &HirProgram,
    typed: &TypedProgram,
    entry: FunctionId,
    files: &SourceFiles,
    last: eml_lir::Pass,
) -> eml_lir::Program {
    let mut program = to_lir(core_until(hir, typed, entry, files, Pass::Evidence));
    eml_lir::passes_until(&mut program, last);
    program
}

/// 誤りを、それを作ったパスの名前で報告する。実行した経路だけでなく、変換のたびに見つけるためである。
fn check(program: &Program, pass: Pass) {
    if !cfg!(debug_assertions) {
        return;
    }
    let result = match pass {
        Pass::Translate | Pass::Optimize => verify_translated(program),
        Pass::Evidence => verify_evidence(program),
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
