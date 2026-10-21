//! 結合テストは、リンクと初回の起動の時間を抑えるため1つのバイナリにまとめる。ここで宣言しないファイルは流れない
//! (docs/implementation/testing.md の「crate の中の置き方」)。

mod common;

mod boxing;
mod census;
mod contract;
mod evidence;
mod externs;
mod instances;
mod local_functions;
mod lowering;
mod optimize;
mod perceus;
mod session;
mod text;
mod translate;
mod unwind;
mod verify;
