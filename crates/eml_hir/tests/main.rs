//! 結合テストは、リンクと初回の起動の時間を抑えるため1つのバイナリにまとめる。ここで宣言しないファイルは流れない
//! (docs/implementation/testing.md の「crate の中の置き方」)。

mod common;

mod classes;
mod data;
mod def_map;
mod effects;
mod eval;
mod externs;
mod item_tree;
mod lists;
mod load;
mod lower;
mod operators;
mod scaling;
mod structure;
mod tuples;
