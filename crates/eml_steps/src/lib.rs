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

pub mod idle;
pub mod limit;
pub mod open;
pub mod plan;
pub mod sleepers;
#[cfg(test)]
mod trust;
pub mod verdict;

verus! {

/// 3つのインタプリタが行うすべての手順と効果。フックとexplorerの共通の字母である。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Label {
    Loop(plan::Step),
    Idle(idle::Effect),
    Verdict(verdict::Effect),
}

impl Label {
    pub const ALL: [Label; 36] = [
        Label::Loop(plan::Step::Stop),
        Label::Loop(plan::Step::Inbox),
        Label::Loop(plan::Step::Root(plan::Root::Main)),
        Label::Loop(plan::Step::Root(plan::Root::Result)),
        Label::Loop(plan::Step::Ready),
        Label::Loop(plan::Step::Detached),
        Label::Loop(plan::Step::Foreign),
        Label::Loop(plan::Step::Look),
        Label::Loop(plan::Step::Idle),
        Label::Loop(plan::Step::Return),
        Label::Idle(idle::Effect::Push),
        Label::Idle(idle::Effect::Probe),
        Label::Idle(idle::Effect::PassOn),
        Label::Idle(idle::Effect::Wake),
        Label::Idle(idle::Effect::IncSleepers),
        Label::Idle(idle::Effect::DecSleepers),
        Label::Idle(idle::Effect::IncSearching),
        Label::Idle(idle::Effect::DecSearching),
        Label::Idle(idle::Effect::Enlist),
        Label::Idle(idle::Effect::Delist),
        Label::Idle(idle::Effect::Scan),
        Label::Idle(idle::Effect::Park),
        Label::Idle(idle::Effect::WakeZero),
        Label::Idle(idle::Effect::Sleep),
        Label::Idle(idle::Effect::Work),
        Label::Verdict(verdict::Effect::Count),
        Label::Verdict(verdict::Effect::Lists),
        Label::Verdict(verdict::Effect::Registry),
        Label::Verdict(verdict::Effect::Own),
        Label::Verdict(verdict::Effect::Wait),
        Label::Verdict(verdict::Effect::WaitUntil),
        Label::Verdict(verdict::Effect::Advance),
        Label::Verdict(verdict::Effect::Restart),
        Label::Verdict(verdict::Effect::RootClosed),
        Label::Verdict(verdict::Effect::Deadlock),
        Label::Verdict(verdict::Effect::Stop),
    ];
}

} // verus!
