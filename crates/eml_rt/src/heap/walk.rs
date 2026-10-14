//! 生存の列挙、リーク、`Heap` を落とすときの後始末
//! (docs/spec/object-model.md の「生存の列挙」)。

use std::collections::BTreeMap;
use std::mem::{self, ManuallyDrop};
use std::sync::Arc;

use vstd::prelude::*;

#[cfg(verus_only)]
use super::Core;
use super::Heap;
use super::check::{ALIVE_STATE, FREED_STATE, UNCOUNTED};
#[cfg(verus_only)]
use super::inv::{p_owner, p_word};
#[cfg(verus_only)]
use super::model::BlockState;
use crate::alloc::FREE_META;
use crate::error::HeapError;
use crate::foreign::{finalizer_of, run_finalizer};
use crate::frozen::Frozen;
#[cfg(verus_only)]
use crate::meta::bits_kind;
use crate::meta::{Header, Kind};
use crate::mode::{Checked, Mode};
use crate::word::Word;

verus! {

/// `h` は、ヘッダの列 `live` のどれかのヘッダの番地である。
pub(super) open spec fn headed(live: Seq<*mut Header>, h: usize) -> bool {
    exists|i: int| #![trigger live[i]] 0 <= i < live.len() && live[i]@.addr == h
}

/// 前置きの状態の欄の、token の値。
const TOKEN_STATE: u32 = 2;

/// 空いた印の `meta` は、種類 `Free` である。
proof fn lemma_free_meta()
    ensures
        bits_kind(FREE_META) == Kind::Free,
{
    assert((0x0F00_0000u32 >> 24u32) & 0xFu32 == 15u32) by (bit_vector);
}

impl<M: Mode> Heap<M> {
    /// ブロック `h` は生きている箱 (token の箱、不死の物体、ダミー、空ける途中の物体を含む) である。
    pub closed spec fn live_block(&self, h: usize) -> bool {
        self.core.dom().contains(h) && (M::CHECKED ==> self.core.state(h) != BlockState::Freed)
    }

    /// 切り出したブロックが生きている箱か。速いモードでは空いた印、検査モードでは前置きの状態で見分ける。
    fn is_live_block(&self, h: *mut Header) -> (r: bool)
        requires
            self.wf(),
            self.core.pages.carved_block(h@.addr),
            h == self.core.pages.block_ptr(h@.addr),
        ensures
            r == self.live_block(h@.addr),
    {
        proof {
            lemma_free_meta();
            if self.core.dom().contains(h@.addr) {
                self.core.lemma_shape(h@.addr);
            }
        }
        if self.core.carved_meta(h) == FREE_META {
            return false;
        }
        if M::CHECKED {
            proof {
                self.core.lemma_code_state(h@.addr);
            }
            self.core.prefix_state_get(h) & 3 != FREED_STATE
        } else {
            true
        }
    }

    /// 生きている箱はヒープのブロックで、生存の状態のブロックは生きている箱である。
    pub(super) proof fn lemma_live_block(&self, h: usize)
        ensures
            self.live_block(h) ==> self.core.dom().contains(h),
            self.core.dom().contains(h) && self.core.state(h) == BlockState::Live
                ==> self.live_block(h),
    {
    }

    /// 凍結の確かめを通るブロック。rc が 0 の `Foreign` でない物体で、検査モードでは生存の状態で、走査する語の行き先も
    /// 生存の状態である。
    pub(super) open spec fn freezable_block(&self, h: usize) -> bool {
        &&& self.core.rc_of(h) == 0
        &&& self.core.kind(h) != Kind::Foreign
        &&& M::CHECKED ==> {
            &&& self.core.state(h) == BlockState::Live
            &&& forall|i: int| #[trigger]
                self.core.scanned(h, i) && p_word(self.core.perm(h), i).is_ptr_spec()
                    ==> self.core.state(p_word(self.core.perm(h), i).bits_spec())
                    == BlockState::Live
        }
    }

    /// 凍結の確かめ (docs/spec/object-model.md の「不死の物体の数え方」)。生きている箱がどれも生存の状態 (空ける途中でも
    /// token でもない) で rc 0 であることを確かめ、そのヘッダを返す。token の箱と `Foreign` の物体は rc 1 なので断る。
    /// 検査モードでは、走査する語が空けたブロックを指していないことも確かめる。速いモードでは、モデルの辺がこれを保証する。
    pub(super) fn freezable(&self) -> (r: Result<Vec<*mut Header>, HeapError>)
        requires
            self.wf(),
            self.core.model@.building is None,
        ensures
            r matches Ok(live) ==> {
                &&& forall|i: int|
                    0 <= i < live@.len() ==> self.live_block(#[trigger] live@[i]@.addr) && live@[i]
                        == self.core.block(live@[i]@.addr)
                &&& forall|h: usize| #[trigger]
                    self.live_block(h) ==> self.freezable_block(h) && headed(live@, h)
            },
    {
        let blocks = self.core.pages.blocks();
        let mut live: Vec<*mut Header> = Vec::new();
        let mut k: usize = 0;
        while k < blocks.len()
            invariant
                self.wf(),
                self.core.model@.building is None,
                k <= blocks@.len(),
                forall|i: int|
                    0 <= i < blocks@.len() ==> self.core.pages.carved_block(
                        #[trigger] blocks@[i]@.addr,
                    ) && blocks@[i] == self.core.pages.block_ptr(blocks@[i]@.addr),
                forall|i: int|
                    0 <= i < live@.len() ==> self.live_block(#[trigger] live@[i]@.addr) && live@[i]
                        == self.core.block(live@[i]@.addr),
                forall|j: int|
                    0 <= j < k && self.live_block(#[trigger] blocks@[j]@.addr)
                        ==> self.freezable_block(blocks@[j]@.addr) && headed(
                        live@,
                        blocks@[j]@.addr,
                    ),
            decreases blocks.len() - k,
        {
            let h = blocks[k];
            let ghost a = h@.addr;
            if self.is_live_block(h) {
                proof {
                    self.core.lemma_shape(a);
                    reveal(Core::building_wf);
                    if M::CHECKED {
                        self.core.lemma_code_state(a);
                    }
                }
                if M::CHECKED && self.core.prefix_state_get(h) & 3 != ALIVE_STATE {
                    return Err(HeapError::NotImmortal);
                }
                if self.core.rc_get(h) != 0 {
                    return Err(HeapError::NotImmortal);
                }
                // `Foreign` の物体は rc 1 で作るので rc の確かめで断るが、塊は値をスレッドの間で共有するので、
                // 種類でも断る

                if matches!(self.core.meta_get(h).kind(), Kind::Foreign) {
                    return Err(HeapError::NotImmortal);
                }
                if M::CHECKED {
                    let (first, n) = self.core.scan_get(h);
                    let mut i: usize = 0;
                    while i < n
                        invariant
                            self.wf(),
                            self.core.model@.building is None,
                            M::CHECKED,
                            self.live_block(a),
                            self.core.state(a) == BlockState::Live,
                            a == h@.addr,
                            h == self.core.block(a),
                            first == super::inv::p_first(self.core.perm(a)),
                            n == super::inv::p_count(self.core.perm(a)),
                            i <= n,
                            forall|j: int|
                                #![trigger p_word(self.core.perm(a), j)]
                                first <= j < first + i && p_word(self.core.perm(a), j).is_ptr_spec()
                                    ==> self.core.state(p_word(self.core.perm(a), j).bits_spec())
                                    == BlockState::Live,
                        decreases n - i,
                    {
                        proof {
                            assert(self.core.scanned(a, (first + i) as int));
                            self.core.lemma_safe_live_slot(a, (first + i) as int);
                        }
                        let child = self.core.slot_get(h, first + i);
                        if child.is_ptr() {
                            let c = self.header_of(child);
                            proof {
                                self.core.lemma_shape(c@.addr);
                                self.core.lemma_code_state(c@.addr);
                                reveal(Core::building_wf);
                            }
                            let cs = self.core.prefix_state_get(c) & 3;
                            if cs != ALIVE_STATE {
                                return Err(
                                    if cs == FREED_STATE {
                                        HeapError::UseAfterFree
                                    } else {
                                        HeapError::NotImmortal
                                    },
                                );
                            }
                        }
                        i += 1;
                    }
                }
                let ghost before = live@;
                live.push(h);
                proof {
                    assert(self.freezable_block(a));
                    assert forall|j: int|
                        0 <= j < k + 1 && self.live_block(
                            #[trigger] blocks@[j]@.addr,
                        ) implies self.freezable_block(blocks@[j]@.addr) && headed(
                        live@,
                        blocks@[j]@.addr,
                    ) by {
                        if j == k {
                            assert(live@[before.len() as int] == h);
                            assert(headed(live@, a));
                        } else {
                            let i = choose|i: int|
                                0 <= i < before.len() && #[trigger] before[i]@.addr
                                    == blocks@[j]@.addr;
                            assert(live@[i] == before[i]);
                        }
                    }
                }
            }
            k += 1;
        }
        proof {
            assert forall|h: usize| #[trigger] self.live_block(h) implies self.freezable_block(h)
                && headed(live@, h) by {
                self.core.pages.lemma_in_use_carved(h);
                let j = choose|j: int| 0 <= j < blocks@.len() && #[trigger] blocks@[j]@.addr == h;
                assert(self.live_block(blocks@[j]@.addr));
            }
        }
        Ok(live)
    }

    /// 生きている物体 (token の箱、不死の物体、ダミーを含む) を列挙する。S11 の根からの走査が使う。
    pub fn live_objects(&self) -> (r: Vec<Word>)
        requires
            self.wf(),
        ensures
            forall|i: int|
                0 <= i < r@.len() ==> (#[trigger] r@[i]).is_ptr_spec() && self.word_in(r@[i])
                    && self.live_block(r@[i].bits_spec()),
            forall|h: usize|
                #![trigger self.live_block(h)]
                self.live_block(h) ==> exists|i: int| 0 <= i < r@.len() && r@[i].bits_spec() == h,
    {
        let blocks = self.core.pages.blocks();
        let mut live: Vec<Word> = Vec::new();
        let mut k: usize = 0;
        while k < blocks.len()
            invariant
                self.wf(),
                k <= blocks@.len(),
                forall|i: int|
                    0 <= i < blocks@.len() ==> self.core.pages.carved_block(
                        #[trigger] blocks@[i]@.addr,
                    ) && blocks@[i] == self.core.pages.block_ptr(blocks@[i]@.addr),
                forall|i: int|
                    0 <= i < live@.len() ==> (#[trigger] live@[i]).is_ptr_spec() && self.word_in(
                        live@[i],
                    ) && self.live_block(live@[i].bits_spec()),
                forall|j: int|
                    0 <= j < k && self.live_block(#[trigger] blocks@[j]@.addr) ==> exists|i: int|
                        0 <= i < live@.len() && live@[i].bits_spec() == blocks@[j]@.addr,
            decreases blocks.len() - k,
        {
            let h = blocks[k];
            if self.is_live_block(h) {
                let w = Word::from_ptr(h);
                proof {
                    self.core.lemma_shape(h@.addr);
                    crate::word::lemma_aligned_is_ptr(h@.addr);
                }
                let ghost before = live@;
                live.push(w);
                proof {
                    assert forall|j: int|
                        0 <= j < k + 1 && self.live_block(
                            #[trigger] blocks@[j]@.addr,
                        ) implies exists|i: int|
                        0 <= i < live@.len() && live@[i].bits_spec() == blocks@[j]@.addr by {
                        if j == k {
                            assert(live@[before.len() as int] == w);
                        } else {
                            let i = choose|i: int|
                                0 <= i < before.len() && before[i].bits_spec() == blocks@[j]@.addr;
                            assert(live@[i] == before[i]);
                        }
                    }
                }
            }
            k += 1;
        }
        proof {
            assert forall|h: usize|
                #![trigger self.live_block(h)]
                self.live_block(h) implies exists|i: int|
                0 <= i < live@.len() && live@[i].bits_spec() == h by {
                self.core.pages.lemma_in_use_carved(h);
                let j = choose|j: int| 0 <= j < blocks@.len() && blocks@[j]@.addr == h;
                assert(self.live_block(blocks@[j]@.addr));
            }
        }
        live
    }

    /// 生きている `Foreign` の物体 (空ける途中の物体を含む) の破棄処理を、1回ずつ呼ぶ。`Heap` を落とすときだけに使う。
    /// Rust の値が持つメモリを漏らさないためである。
    pub(super) fn finalize_all(&self)
        requires
            self.wf(),
    {
        let blocks = self.core.pages.blocks();
        let mut k: usize = 0;
        while k < blocks.len()
            invariant
                self.wf(),
                k <= blocks@.len(),
                forall|i: int|
                    0 <= i < blocks@.len() ==> self.core.pages.carved_block(
                        #[trigger] blocks@[i]@.addr,
                    ) && blocks@[i] == self.core.pages.block_ptr(blocks@[i]@.addr),
            decreases blocks.len() - k,
        {
            let h = blocks[k];
            if self.is_live_block(h) {
                proof {
                    self.core.lemma_shape(h@.addr);
                }
                if self.core.meta_get(h).has_finalizer() {
                    let tracked p = self.core.perms.borrow().tracked_borrow(h@.addr);
                    let f = finalizer_of(h, Tracked(p));
                    run_finalizer(f, Tracked(p));
                }
            }
            k += 1;
        }
    }

    /// 生きている箱ごとの、リークの件数。種類が `None` なら token の箱である。不死の物体は外に出ている参照の数を
    /// 件数にし、検査モードでなければ件数は数えない。
    pub(super) fn leak_entries(&self) -> (r: Vec<(Option<Kind>, u64)>)
        requires
            self.wf(),
            M::CHECKED,
    {
        let blocks = self.core.pages.blocks();
        let mut out: Vec<(Option<Kind>, u64)> = Vec::new();
        let mut k: usize = 0;
        while k < blocks.len()
            invariant
                self.wf(),
                M::CHECKED,
                k <= blocks@.len(),
                forall|i: int|
                    0 <= i < blocks@.len() ==> self.core.pages.carved_block(
                        #[trigger] blocks@[i]@.addr,
                    ) && blocks@[i] == self.core.pages.block_ptr(blocks@[i]@.addr),
            decreases blocks.len() - k,
        {
            let h = blocks[k];
            if self.is_live_block(h) {
                proof {
                    self.core.lemma_shape(h@.addr);
                }
                let state = self.core.prefix_state_get(h) & 3;
                if state == TOKEN_STATE {
                    out.push((None, 1));
                } else {
                    let count = if self.core.rc_get(h) == 0 {
                        let n = self.core.prefix_first_get(h);
                        if n == UNCOUNTED {
                            0
                        } else {
                            n as u64
                        }
                    } else {
                        1
                    };
                    out.push((Some(self.core.meta_get(h).kind()), count));
                }
            }
            k += 1;
        }
        out
    }
}

impl<M: Mode> Heap<M> {
    /// 生きている物体 (構築中と空ける途中の物体を含む) か。前置きの状態が token と解放のどちらでもない。
    closed spec fn alive(&self, h: usize) -> bool {
        &&& self.core.dom().contains(h)
        &&& self.core.state(h) != BlockState::Freed
        &&& self.core.state(h) != BlockState::Token
    }

    /// 生きている物体 `b` の i 語目の子は、生存の列挙の不変条件を満たす。子は、生きている物体か付けた塊の物体である。
    closed spec fn child_ok(&self, b: usize, i: int) -> bool {
        let w = p_word(self.core.perm(b), i);
        let c = w.bits_spec();
        w.is_ptr_spec() ==> {
            &&& self.alive(c) || self.core.in_region(c)
            &&& self.core.rc_of(b) == 0 ==> self.core.rc_of(c) == 0
            &&& self.core.rc_of(b) < 0 && self.core.rc_of(c) > 0 ==> self.core.lin(c)
        }
    }

    /// 生存の列挙の不変条件。局所でない生きている物体の子は、生きていて、不死の物体の子は不死で、共有の物体の局所の子は
    /// `lin` ビットを持つ。
    pub closed spec fn reach_ok(&self) -> bool {
        forall|b: usize, i: int|
            #![trigger self.core.scanned(b, i)]
            self.alive(b) && self.core.rc_of(b) <= 0 && self.core.scanned(b, i) ==> self.child_ok(
                b,
                i,
            )
    }

    /// 生存の列挙で見つかる不変条件の違反 (docs/spec/object-model.md の「報告する誤り」)。共有の物体から届いてよいのは
    /// 共有か不死の物体か `lin` ビットの立った局所の物体で、不死の物体から届いてよいのは不死の物体だけである。
    pub(super) fn check_reach(&self) -> (r: Result<(), HeapError>)
        requires
            self.wf(),
            self.safe_settleable(),
            M::CHECKED,
        ensures
            r is Ok <==> self.reach_ok(),
    {
        let blocks = self.core.pages.blocks();
        let mut k: usize = 0;
        while k < blocks.len()
            invariant
                self.wf(),
                self.safe_settleable(),
                M::CHECKED,
                k <= blocks@.len(),
                forall|i: int|
                    0 <= i < blocks@.len() ==> self.core.pages.carved_block(
                        #[trigger] blocks@[i]@.addr,
                    ) && blocks@[i] == self.core.pages.block_ptr(blocks@[i]@.addr),
                forall|j: int, i: int|
                    #![trigger self.core.scanned(blocks@[j]@.addr, i)]
                    0 <= j < k && self.alive(blocks@[j]@.addr) && self.core.rc_of(blocks@[j]@.addr)
                        <= 0 && self.core.scanned(blocks@[j]@.addr, i) ==> self.child_ok(
                        blocks@[j]@.addr,
                        i,
                    ),
            decreases blocks.len() - k,
        {
            let h = blocks[k];
            let ghost a = h@.addr;
            proof {
                lemma_free_meta();
                if self.core.dom().contains(a) {
                    self.core.lemma_shape(a);
                    self.core.lemma_code_state(a);
                }
            }
            if self.core.carved_meta(h) != FREE_META {
                let state = self.core.prefix_state_get(h) & 3;
                if state != FREED_STATE && state != TOKEN_STATE {
                    let rc = self.core.rc_get(h);
                    if rc <= 0 {
                        proof {
                            self.core.lemma_safe_at(a);
                        }
                        let (first, n) = self.core.scan_get(h);
                        let mut i: usize = 0;
                        while i < n
                            invariant
                                self.wf(),
                                self.safe_settleable(),
                                M::CHECKED,
                                self.alive(a),
                                a == h@.addr,
                                h == self.core.block(a),
                                rc == self.core.rc_of(a),
                                rc <= 0,
                                first == super::inv::p_first(self.core.perm(a)),
                                n == super::inv::p_count(self.core.perm(a)),
                                i <= n,
                                forall|j: int|
                                    #![trigger self.child_ok(a, j)]
                                    first <= j < first + i ==> self.child_ok(a, j),
                            decreases n - i,
                        {
                            proof {
                                assert(self.core.scanned(a, (first + i) as int));
                                self.core.lemma_safe_live_slot(a, (first + i) as int);
                            }
                            let child = self.core.slot_get(h, first + i);
                            if child.is_ptr() {
                                let c = self.header_of(child);
                                let ghost b = c@.addr;
                                proof {
                                    self.core.lemma_shape(b);
                                    self.core.lemma_code_state(b);
                                    if !self.core.dom().contains(b) {
                                        self.core.lemma_region_at(b);
                                    }
                                }
                                let cs = self.core.prefix_state_get(c) & 3;
                                if cs == FREED_STATE || cs == TOKEN_STATE {
                                    proof {
                                        assert(!self.child_ok(a, (first + i) as int));
                                    }
                                    return Err(HeapError::UseAfterFree);
                                }
                                let crc = self.core.rc_get(c);
                                if rc == 0 && crc != 0 {
                                    proof {
                                        assert(!self.child_ok(a, (first + i) as int));
                                    }
                                    return Err(HeapError::ImmortalReachesMortal);
                                }
                                if rc < 0 && crc > 0 && !self.core.meta_get(c).lin() {
                                    proof {
                                        assert(!self.child_ok(a, (first + i) as int));
                                    }
                                    return Err(HeapError::SharedReachesLocal);
                                }
                            }
                            proof {
                                assert(self.child_ok(a, (first + i) as int));
                            }
                            i += 1;
                        }
                    }
                }
            }
            k += 1;
        }
        proof {
            assert forall|b: usize, i: int|
                #![trigger self.core.scanned(b, i)]
                self.alive(b) && self.core.rc_of(b) <= 0 && self.core.scanned(
                    b,
                    i,
                ) implies self.child_ok(b, i) by {
                self.core.pages.lemma_in_use_carved(b);
                let j = choose|j: int| 0 <= j < blocks@.len() && blocks@[j]@.addr == b;
                assert(self.core.scanned(blocks@[j]@.addr, i));
            }
        }
        Ok(())
    }
}

/// 付けていた塊の `Arc` を手放す。`Heap` を落とすときに、後始末の後で呼ぶ。
// 呼ぶ側に展開させない理由は、`Heap` の `region` の欄の注釈にある
#[inline(never)]
pub(super) fn release_region<M: Mode>(region: Option<Arc<Frozen<M>>>) {
    let _ = region;
}

} // verus!

impl Heap<Checked> {
    /// 生存の列挙で見つかる不変条件の違反 (docs/spec/object-model.md の「報告する誤り」)。
    pub fn check_invariants(&self) -> Result<(), HeapError> {
        self.check_reach()
    }

    /// 種類の名前ごとのリーク (docs/spec/object-model.md の「報告する誤り」)。不死の物体は外に出ている参照の数を足し、
    /// token の箱は `Token` で数える。
    pub fn leaks(&self) -> BTreeMap<&'static str, u64> {
        let mut leaks = BTreeMap::new();
        for (kind, count) in self.leak_entries() {
            if count > 0 {
                *leaks.entry(kind.map_or("Token", Kind::name)).or_default() += count;
            }
        }
        leaks
    }
}

impl<M: Mode> Drop for Heap<M> {
    fn drop(&mut self) {
        self.finalize_all();
        release_region(ManuallyDrop::into_inner(mem::replace(
            &mut self.region,
            ManuallyDrop::new(None),
        )));
    }
}
