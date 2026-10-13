//! 速いモードと検査モード (docs/spec/object-model.md の「モードの選び方」)。
//! 型の引数で選ぶので、速いモードでは検査の分岐が単相化で消える。

use vstd::prelude::*;

verus! {

mod sealed {
    pub trait Sealed {

    }

}

pub trait Mode: sealed::Sealed + 'static {
    const CHECKED: bool;
}

// Verus は空の enum を受け付けないので、中身のない struct にする
#[derive(Debug)]
pub struct Fast;

#[derive(Debug)]
pub struct Checked;

impl sealed::Sealed for Fast {

}

impl sealed::Sealed for Checked {

}

impl Mode for Fast {
    const CHECKED: bool = false;
}

impl Mode for Checked {
    const CHECKED: bool = true;
}

} // verus!
