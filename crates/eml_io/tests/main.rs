//! `eml_io` の結合テスト。1つのバイナリ `integration` にまとめる (docs/implementation/testing.md の「crate の中の置き方」)。

mod real;
mod test_driver;
#[cfg(feature = "tls")]
mod tls;
// tls を外した振る舞いは OS で分けない (docs/implementation/testing.md の「feature の組み合わせの確認」)
#[cfg(not(feature = "tls"))]
mod tls_off;
