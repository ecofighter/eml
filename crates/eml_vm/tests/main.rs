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
mod io;
mod machine;
mod miri;
mod par;
mod preempt;
mod reuse;
mod roots;
mod run;
mod scaling;
mod signal;
mod structs;
mod tags;
mod tasks;
// tls を外した振る舞いは OS で分けない (docs/implementation/testing.md の「feature の組み合わせの確認」)
#[cfg(not(feature = "tls"))]
mod tls_off;
mod variants;
