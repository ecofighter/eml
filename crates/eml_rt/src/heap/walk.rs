//! 生存の列挙、リーク、`Heap` を落とすときの後始末
//! (docs/superpowers/specs/2026-10-13-s9a-eml-rt-core-design.md の「生存の列挙」)。

use std::collections::BTreeMap;
use std::ptr::NonNull;

use super::Heap;
use super::check::{self, State};
use crate::foreign;
use crate::meta::{self, Header, Kind};
use crate::mode::{Checked, Mode};
use crate::sync::Ordering;
use crate::word::Word;

impl<M: Mode> Heap<M> {
    /// 生きている物体 (token の箱、不死の物体、ダミーを含む) を列挙する。S11 の根からの走査が使う。
    pub fn live_objects(&self) -> Vec<Word> {
        let mut live = Vec::new();
        self.pages.for_each_block(|h| {
            if unsafe { self.is_live_block(h) } {
                live.push(Word::from_header(h));
            }
        });
        live
    }

    /// 切り出したブロックが生きている箱か。速いモードでは種類 `Free` の印、検査モードでは前置きの状態で見分ける。
    pub(super) unsafe fn is_live_block(&self, h: NonNull<Header>) -> bool {
        if M::CHECKED {
            unsafe { check::prefix(h) }.state() != State::Freed
        } else {
            unsafe { meta::meta(h) }.kind() != Kind::Free
        }
    }
}

impl Heap<Checked> {
    /// 種類の名前ごとのリーク (spec の「報告する誤り」)。不死の物体は外に出ている参照の数を足し、token の箱は
    /// `Token` で数える。
    pub fn leaks(&self) -> BTreeMap<&'static str, u64> {
        let mut leaks = BTreeMap::new();
        self.pages.for_each_block(|h| {
            let prefix = unsafe { check::prefix(h) };
            let (name, count) = match prefix.state() {
                State::Freed => return,
                State::Token => ("Token", 1),
                State::Alive => {
                    let count = if unsafe { meta::rc(h) }.load(Ordering::Relaxed) == 0 {
                        match prefix.first.load(Ordering::Relaxed) {
                            check::UNCOUNTED => 0,
                            n => u64::from(n),
                        }
                    } else {
                        1
                    };
                    (unsafe { meta::meta(h) }.kind().name(), count)
                }
            };
            if count > 0 {
                *leaks.entry(name).or_default() += count;
            }
        });
        leaks
    }
}

impl<M: Mode> Drop for Heap<M> {
    /// Rust の値が持つメモリを漏らさないように、生きている `Foreign` の破棄処理を呼んでからページを返す。
    fn drop(&mut self) {
        let mut finalizable = Vec::new();
        self.pages.for_each_block(|h| {
            if unsafe { self.is_live_block(h) && meta::meta(h).kind() == Kind::Foreign } {
                finalizable.push(h);
            }
        });
        for h in finalizable {
            unsafe { foreign::finalizer(h).run() };
        }
    }
}
