//! スケジューラの判断を、Verusで証明した関数として持つ (docs/implementation/architecture.mdの「`eml_steps`の内部」)。

// Verusは、仕様のないclosureを`Option::map`に渡すと中を見通せないので、`match`で書く。`new`と`len`はスケジューラの使う
// 操作で、`Default`と`is_empty`を足すと使い手のない項目になる
#![allow(
    clippy::manual_map,
    clippy::new_without_default,
    clippy::len_without_is_empty
)]

// Verusはcrateの根にvstdのpreludeを求める
#[allow(unused_imports)]
use vstd::prelude::*;

pub mod limit;
pub mod open;
pub mod sleepers;
#[cfg(test)]
mod trust;
