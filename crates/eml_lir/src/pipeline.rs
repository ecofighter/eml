//! LIR のパスの順番 (docs/spec/lir.md の「パスの順番」)。LIR の層の順番を知っているのはこのファイルだけにする。
//! Core IR から LIR への変換までの順番は `eml_core_ir` が知る。

use crate::{
    Program, VerifyError, boxing, contract, flatten, perceus, reuse, verify, verify_flattened,
    verify_lowered, verify_scopes,
};

/// `passes_until` で止める位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pass {
    Boxing,
    Flatten,
    Contract,
    Perceus,
    Reuse,
}

impl Pass {
    fn name(self) -> &'static str {
        match self {
            Pass::Boxing => "boxing",
            Pass::Flatten => "flatten",
            Pass::Contract => "contract",
            Pass::Perceus => "perceus",
            Pass::Reuse => "reuse",
        }
    }
}

type Run = fn(&mut Program);
type Check = fn(&Program) -> Result<(), VerifyError>;

/// パスの順番。パスごとに、その後に debug ビルドでかける verifier を持つ。
const PASSES: [(Pass, Run, Check); 5] = [
    (Pass::Boxing, boxing, verify_scopes),
    (Pass::Flatten, flatten, verify_flattened),
    (Pass::Contract, contract, verify_flattened),
    (Pass::Perceus, perceus, verify),
    (Pass::Reuse, reuse, verify),
];

/// 変換の直後の LIR に、box の挿入から `last` までをかける。入力の約束 (`tail`、`box`、`unbox`、RC の命令が
/// ないこと) を入口で確かめるので、手で書いた LIR を渡すテストもこの検査を通る
/// (docs/spec/lir.md の「パスの順番」)。検査は debug ビルドでかける。
pub fn passes_until(program: &mut Program, last: Pass) {
    if cfg!(debug_assertions)
        && let Err(error) = verify_lowered(program)
    {
        panic!("internal error: invalid LIR before boxing: {error}");
    }
    for (pass, run, check) in PASSES {
        run(program);
        if cfg!(debug_assertions)
            && let Err(error) = check(program)
        {
            panic!("internal error: invalid LIR after {}: {error}", pass.name());
        }
        if pass == last {
            return;
        }
    }
}
