//! 結合テストは、リンクと初回の起動の時間を抑えるため1つのバイナリにまとめる。ここで宣言しないファイルは流れない
//! (docs/implementation/testing.md の「crate の中の置き方」)。

mod common;

mod closures;
mod data;
mod run;
mod scaling;
mod v2_closures;
mod v2_data;
mod v2_run;
