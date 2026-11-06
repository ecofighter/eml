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
mod ffi;
mod ffi_abandon;
mod io;
mod machine;
mod memory;
mod miri;
mod par;
mod reuse;
mod roots;
mod run;
mod scaling;
mod seat;
mod signal;
mod spawns;
mod structs;
mod tags;
mod tasks;
// tls を外した振る舞いは OS で分けない (docs/implementation/testing.md の「feature の組み合わせの確認」)
#[cfg(not(feature = "tls"))]
mod tls_off;
mod variants;
mod workers;
