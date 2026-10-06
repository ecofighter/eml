//! Core IR のパスの順番 (docs/spec/core-ir.md)。順番を知っているのはこのファイルだけにする。テストは `lower_until`
//! で、確かめたいパスの直後の IR を見る。

use eml_hir::{FunctionId, Program as HirProgram};
use eml_types::TypedProgram;

use crate::{
    Program, VerifyError, compact, liveness, perceus, simplify, translate, verify, verify_scopes,
};

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
///
/// `entry` は本体を持つ関数 (intrinsic ではない) で、型が `Unit -> ...` か、引数がなく `Unit -> ...` の関数を返すものでなければならない。
/// `main` は型検査器が E2004 で保証するが、ほかの呼び出し元 (将来の REPL など) は自分で保証する。
pub fn lower(hir: &HirProgram, typed: &TypedProgram, entry: FunctionId) -> Program {
    lower_until(hir, typed, entry, Pass::Perceus)
}

/// `last` の直後で止める。止めたパスまでの検査は、debug ビルドでかける。
pub fn lower_until(
    hir: &HirProgram,
    typed: &TypedProgram,
    entry: FunctionId,
    last: Pass,
) -> Program {
    let mut program = translate::translate(hir, typed, entry);
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
    compact_all(&mut program, Pass::Perceus);
    check(&program, Pass::Perceus);
    program
}

/// パスの中では `captures` が古くなってよいが、パスの間ではつねに正しくする (docs/spec/core-ir.md のパスの表)。
/// 途中で止めた IR の表示と、`verify_scopes` が `captures` を宣言として扱うためである。
fn settle(program: &mut Program, pass: Pass) {
    compact_all(program, pass);
    for function in &mut program.functions {
        liveness::analyze(function);
    }
    check(program, pass);
}

/// パスの後でアリーナを根からの前順に組み直す。たどれない式と消えた join point を捨て、次のパスと verifier が
/// アリーナ全体を1本の木として扱えるようにする (docs/spec/core-ir.md のパスの表)。`compact` が見つけた木の誤りは、
/// `check` と違ってどのビルドでも、パスの名前で報告する。理由は compact.rs に書いた。
fn compact_all(program: &mut Program, pass: Pass) {
    for function in &mut program.functions {
        if let Err(message) = compact::compact(function) {
            fail(
                pass,
                VerifyError {
                    function: function.name.clone(),
                    message,
                },
            );
        }
    }
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
        fail(pass, error);
    }
}

fn fail(pass: Pass, error: VerifyError) -> ! {
    panic!(
        "internal error: invalid Core IR after {}: {error}",
        pass.name()
    );
}
