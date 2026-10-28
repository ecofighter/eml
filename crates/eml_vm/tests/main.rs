//! 結合テストは、リンクと初回の起動の時間を抑えるため1つのバイナリにまとめる。ここで宣言しないファイルは流れない
//! (docs/implementation/testing.md の「crate の中の置き方」)。

mod common;

mod api;
mod apply;
mod bench;
mod calls;
mod chan;
mod codegen;
mod data;
mod evidence;
mod machine;
mod miri;
mod par;
mod preempt;
mod reuse;
mod roots;
mod run;
mod scaling;
mod structs;
mod tags;
mod tasks;
mod variants;
