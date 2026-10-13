//! 速いモードと検査モード (docs/spec/object-model.md の「モードの選び方」)。
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
