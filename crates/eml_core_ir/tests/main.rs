//! 結合テストは、リンクと初回の起動の時間を抑えるため1つのバイナリにまとめる。ここで宣言しないファイルは流れない
//! (docs/implementation/testing.md の「crate の中の置き方」)。

mod common;

mod boxing;
mod contract;
mod evidence;
mod externs;
mod instances;
mod lowering;
mod perceus;
mod s8a_baseline;
mod text;
mod translate;
mod verify;
