//! 結合テストは、リンクと初回の起動の時間を抑えるため1つのバイナリにまとめる。ここで宣言しないファイルは流れない
//! (docs/implementation/testing.md の「crate の中の置き方」)。

mod common;

mod contract;
mod externs;
mod perceus;
mod text;
mod translate;
mod verify;
