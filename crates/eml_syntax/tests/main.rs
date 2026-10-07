//! 結合テストは、リンクと初回の起動の時間を抑えるため1つのバイナリにまとめる。ここで宣言しないファイルは流れない
//! (docs/implementation/testing.md の「crate の中の置き方」)。

mod common;

mod ast;
mod control;
mod corpus;
mod declarations;
mod expressions;
mod handlers;
mod lexer;
mod literals;
mod names;
mod nesting;
mod operators;
mod parser;
mod types;
