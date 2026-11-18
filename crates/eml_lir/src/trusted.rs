use std::ops::Deref;

use crate::Program;

/// エンジンが実行してよいと約束されたLIR。エンジンのエントリはこの型だけを受け取る (docs/spec/lir.mdの「エンジンの約束」)。
/// 内側の`Program`を書き換える道は作らない: 約束は作った時点の中身についてだけ成り立つ。
#[derive(Debug, Clone)]
pub struct Trusted(Program);

impl Trusted {
    /// # Safety
    ///
    /// `program`はdocs/spec/lir.mdの「実行の前提」のうちverifierが確かめられない前提をすべて満たす (約束P1、
    /// docs/spec/lir.mdの「エンジンの約束」)。
    pub unsafe fn new(program: Program) -> Trusted {
        Trusted(program)
    }
}

impl Deref for Trusted {
    type Target = Program;

    fn deref(&self) -> &Program {
        &self.0
    }
}
