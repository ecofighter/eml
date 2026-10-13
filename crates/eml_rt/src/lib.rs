//! VM とネイティブで共有するランタイムの核 (docs/superpowers/specs/2026-10-13-s9a-eml-rt-core-design.md)。語、ヘッダ、
//! 確保器、RC、`share`、reuse の部品、検査モードを持つ。まだどの crate からも使わない。

mod alloc;
mod meta;
mod mode;
mod rc;
mod sync;
mod word;

pub use meta::{Kind, Meta};
pub use mode::{Checked, Fast, Mode};
pub use word::Word;

#[cfg(test)]
mod tests {
    use super::*;

    fn send_and_sync<T: Send + Sync>() {}

    #[test]
    fn words_cross_threads() {
        send_and_sync::<Word>();
    }
}
