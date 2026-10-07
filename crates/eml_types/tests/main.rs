//! 結合テストは、リンクと初回の起動の時間を抑えるため1つのバイナリにまとめる。ここで宣言しないファイルは流れない
//! (docs/implementation/testing.md の「crate の中の置き方」)。

mod common;

mod check;
mod data;
mod effects;
mod exhaustive;
mod linearity;
mod modules;
mod rows;
mod scaling;
mod tuples;
