//! LIR のパスの順番 (docs/spec/lir.md の「パスの順番」)。LIR の層の順番を知っているのはこのファイルだけにする。
//! Core IR から LIR への変換までの順番は `eml_core_ir` が知る。

use crate::{
    Program, VerifyError, boxing, contract, perceus, verify, verify_lowered, verify_scopes,
};

/// `passes_until` で止める位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pass {
    Boxing,
    Contract,
    Perceus,
}

impl Pass {
    fn name(self) -> &'static str {
        match self {
            Pass::Boxing => "boxing",
            Pass::Contract => "contract",
            Pass::Perceus => "perceus",
        }
    }
}

/// 変換の直後の LIR に、box の挿入から `last` までをかける。入力の約束 (`tail`、`box`、`unbox`、RC の命令が
/// ないこと) を入口で確かめるので、手で書いた LIR を渡すテストもこの検査を通る
/// (docs/spec/lir.md の「パスの順番」)。検査は debug ビルドでかける。
pub fn passes_until(program: &mut Program, last: Pass) {
    if cfg!(debug_assertions)
        && let Err(error) = verify_lowered(program)
    {
        panic!("internal error: invalid LIR before boxing: {error}");
    }
    boxing(program);
    check(program, Pass::Boxing);
    if last == Pass::Boxing {
        return;
    }
    contract(program);
    check(program, Pass::Contract);
    if last == Pass::Contract {
        return;
    }
    perceus(program);
    check(program, Pass::Perceus);
}

/// 誤りを、それを作ったパスの名前で報告する。
fn check(program: &Program, pass: Pass) {
    if !cfg!(debug_assertions) {
        return;
    }
    let result = match pass {
        Pass::Boxing | Pass::Contract => verify_scopes(program),
        Pass::Perceus => verify(program),
    };
    if let Err(error) = result {
        fail(pass, error);
    }
}

fn fail(pass: Pass, error: VerifyError) -> ! {
    panic!("internal error: invalid LIR after {}: {error}", pass.name());
}
