//! `eml_rt` の純粋な関数と `share` のたどりの Verus の写し (spec の「Verus」)。Cargo のビルドには入らず、`check.sh` で流す。
//! 写しは本体と同じコードに仕様と証明を足したもので、本体と同じことは `crates/eml_rt/src/verus_copies.rs` のテストが確かめる。

mod check;
mod meta;
mod rc;
mod share;
mod word;
