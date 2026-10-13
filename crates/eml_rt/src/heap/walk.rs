//! 生存の列挙、リーク、`Heap` を落とすときの後始末
//! (docs/spec/object-model.md の「生存の列挙」)。

use std::collections::BTreeMap;
use std::ptr::NonNull;

use super::check::{self, State};
use super::{Heap, scan_count, scan_slot};
use crate::error::HeapError;
use crate::foreign;
use crate::meta::{self, Header, Kind};
use crate::mode::{Checked, Mode};
use crate::word::Word;
use std::sync::atomic::Ordering;

impl<M: Mode> Heap<M> {
    /// 切り出したすべてのブロックについて `f` を呼ぶ。
    pub(super) fn each_block(&self, mut f: impl FnMut(NonNull<Header>)) {
        for h in self.core.pages.blocks() {
            // 切り出したブロックはページの中にあるので、null にならない
            f(unsafe { NonNull::new_unchecked(h) })
        }
    }

    /// 生きている物体 (token の箱、不死の物体、ダミーを含む) を列挙する。S11 の根からの走査が使う。
    pub fn live_objects(&self) -> Vec<Word> {
        let mut live = Vec::new();
        self.each_block(|h| {
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
    /// 生存の列挙で見つかる不変条件の違反 (docs/spec/object-model.md の「報告する誤り」)。共有の物体から届いてよいのは共有か不死の物体か
    /// `lin` ビットの立った局所の物体で、不死の物体から届いてよいのは不死の物体だけである。
    pub fn check_invariants(&self) -> Result<(), HeapError> {
        let mut result = Ok(());
        self.each_block(|h| {
            if result.is_err() || unsafe { check::prefix(h) }.state() != State::Alive {
                return;
            }
            let rc = unsafe { meta::rc(h) }.load(Ordering::Relaxed);
            if rc > 0 {
                return;
            }
            for i in 0..unsafe { scan_count(h) } {
                let child = unsafe { scan_slot(h, i).read() };
                if !child.is_ptr() {
                    continue;
                }
                let c = child.header();
                if unsafe { check::prefix(c) }.state() != State::Alive {
                    result = Err(HeapError::UseAfterFree);
                    return;
                }
                let child_rc = unsafe { meta::rc(c) }.load(Ordering::Relaxed);
                if rc == 0 && child_rc != 0 {
                    result = Err(HeapError::ImmortalReachesMortal);
                    return;
                }
                if rc < 0 && child_rc > 0 && !unsafe { meta::meta(c) }.lin() {
                    result = Err(HeapError::SharedReachesLocal);
                    return;
                }
            }
        });
        result
    }

    /// 種類の名前ごとのリーク (docs/spec/object-model.md の「報告する誤り」)。不死の物体は外に出ている参照の数を足し、token の箱は
    /// `Token` で数える。
    pub fn leaks(&self) -> BTreeMap<&'static str, u64> {
        let mut leaks = BTreeMap::new();
        self.each_block(|h| {
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
        self.each_block(|h| {
            if unsafe { self.is_live_block(h) && meta::meta(h).kind() == Kind::Foreign } {
                finalizable.push(h);
            }
        });
        for h in finalizable {
            unsafe { foreign::finalizer(h).run() };
        }
    }
}
