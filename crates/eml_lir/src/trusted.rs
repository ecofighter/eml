use std::ops::Deref;

use crate::unwind::{self, Unwinds};
use crate::{Program, VerifyError};

/// エンジンが実行してよいと約束されたLIRと、その関数ごとの巻き戻しの表。エンジンのエントリはこの型だけを受け取る
/// (docs/spec/lir.mdの「エンジンの約束」)。
/// 内側の`Program`を書き換える道は作らない: 約束は作った時点の中身についてだけ成り立つ。
#[derive(Debug, Clone)]
pub struct Trusted {
    program: Program,
    unwinds: Vec<Unwinds>,
}

impl Trusted {
    /// どのビルドでも、プログラムを所有の段のverifierで確かめ、同じ走査で関数ごとの巻き戻しの表を作る。verifierが拒む
    /// プログラムは、その誤りで断る。
    ///
    /// # Safety
    ///
    /// `program`はdocs/spec/lir.mdの「実行の前提」のうちverifierが確かめられない前提をすべて満たす (約束P1、
    /// docs/spec/lir.mdの「エンジンの約束」)。
    pub unsafe fn new(program: Program) -> Result<Trusted, VerifyError> {
        let unwinds = unwind::verified(&program)?;
        Ok(Trusted { program, unwinds })
    }

    /// verifierを走らせずにプログラムを包む。所有の段の走査を通る関数はその表を持ち、走査が拒む関数は何も手放さない表
    /// (`unwind::unowned`) を持つ。誤りをわざと入れたIRを走らせるテストのためだけにある。
    ///
    /// # Safety
    ///
    /// 約束P6 (docs/spec/lir.mdの「エンジンの約束」): `program`は、呼ぶ側がわざと入れた違反を除き、verifierが確かめる
    /// 規則を含めて「実行の前提」をすべて満たす。わざと入れた違反はどれも、値を読み込むエンジンとモードが、操作が
    /// それに頼る前に報告するものか、物体を手放さずに残すだけのものである。
    pub unsafe fn unverified(program: Program) -> Trusted {
        let unwinds = unwind::points(&program)
            .into_iter()
            .zip(&program.functions)
            .map(|(table, function)| {
                table.unwrap_or_else(|_| unwind::unowned(function, &program.c_functions))
            })
            .collect();
        Trusted { program, unwinds }
    }

    /// 関数ごとの巻き戻しの表。`FnIdx`の番号で引く。
    pub fn unwinds(&self) -> &[Unwinds] {
        &self.unwinds
    }
}

impl Deref for Trusted {
    type Target = Program;

    fn deref(&self) -> &Program {
        &self.program
    }
}
