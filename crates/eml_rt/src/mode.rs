//! 速いモードと検査モード (docs/superpowers/specs/2026-10-13-s9a-eml-rt-core-design.md の「モードの選び方」)。
//! 型の引数で選ぶので、速いモードでは検査の分岐が単相化で消える。

mod sealed {
    pub trait Sealed {}
}

pub trait Mode: sealed::Sealed + 'static {
    const CHECKED: bool;
}

#[derive(Debug)]
pub enum Fast {}

#[derive(Debug)]
pub enum Checked {}

impl sealed::Sealed for Fast {}
impl sealed::Sealed for Checked {}

impl Mode for Fast {
    const CHECKED: bool = false;
}

impl Mode for Checked {
    const CHECKED: bool = true;
}
