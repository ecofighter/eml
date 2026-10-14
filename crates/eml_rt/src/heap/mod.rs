//! ヒープ: 確保、RC、解放の連鎖、フィールドの読み書き
//! (docs/spec/object-model.md の「核の操作」)。語を受ける操作と、ペイロードを
//! 呼ぶ側が書く `alloc` と `alloc_immortal` は `unsafe fn` で、安全の条件は docs/spec/object-model.md の「核の操作」にある。その条件の
//! 「生きている物体」は、`meta` に合うペイロード (走査する語、`scan` が 255 なら数の語、`BigInt` なら値) をすべて書いた
//! 物体を指す。データの生の語はこの条件に入らない。検査モードは、確かめられる違反を `Err` で返す。各メソッドの
//! `# Safety` には、速いモードが確かめず検査モードが `Err` にする条件も書く。

mod chain;
pub(crate) mod check;
mod core;
mod field;
mod handover;
mod inv;
mod model;
mod proof;
mod release;
mod reuse;
mod safe;
pub(crate) mod share;
mod states;
mod string;
mod walk;

#[cfg(test)]
mod tests;

use std::marker::PhantomData;
use std::mem;

use crate::alloc::FREE_META;
use crate::alloc::{Pages, too_large};
use crate::error::HeapError;
use crate::foreign;
use crate::meta::{self, BlockPerm, HEADER_WORDS, Header, Kind, Meta, meta_ptr, rc_ptr, slot_ptr};
#[cfg(verus_only)]
use crate::meta::{bits_kind, bits_scan, bits_tag, bits_wf};
use crate::mode::Mode;
use crate::rc::{self, DecrefStep, DupStep};
#[cfg(verus_only)]
use crate::rc::{Obj, decref_next};
use crate::word::Word;
use vstd::prelude::*;
use vstd::raw_ptr::ptr_mut_write;

pub use reuse::Token;

use crate::foreign::{finalizer_of, run_finalizer};
#[cfg(verus_only)]
use crate::word::lemma_aligned_is_ptr;
#[cfg(verus_only)]
use chain::listed;
#[cfg(verus_only)]
use check::ALIVE_STATE;
use check::{PREFIX_WORDS, UNCOUNTED, init_prefix, mark_freed};
#[cfg(verus_only)]
use field::field_edges;
#[cfg(verus_only)]
use handover::handover;
#[cfg(verus_only)]
use inv::{
    p_code, p_count, p_first, p_init, p_kind, p_meta, p_owner, p_scanned, p_word, state_code,
};
#[cfg(verus_only)]
use model::BlockState;
use model::{Model, Pos};

verus! {

/// 検査モードだけで数える仕事の回数 (docs/spec/object-model.md の「モードの選び方」)。ダミーの1つも数に入る。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Stats {
    pub rc_increments: u64,
    pub rc_decrements: u64,
    pub allocations: u64,
    /// 生きている箱 (token の箱を含む) の数。
    pub live: u64,
    pub peak_live: u64,
}

/// ヒープの証明の対象になる部分。不変条件はこの部分の上で書くので、`Heap` のほかの欄 (数の記録、作業リストなど) を
/// 書き換えても不変条件はそのまま保たれる。
// ghost の欄は Verus の外のビルドでは読まないので、使わないことの警告を止める
#[cfg_attr(not(verus_keep_ghost), allow(dead_code))]
pub(crate) struct Core {
    pub(crate) pages: Pages,
    /// 呼ぶ側に渡したブロック (生きている物体、構築中の物体、検査モードの隔離したブロック) の権限。
    pub(crate) perms: Tracked<Map<usize, BlockPerm>>,
    pub(crate) model: Ghost<Model>,
    /// 検査モードか。`Heap` の `M::CHECKED` に等しい。
    pub(crate) checked: Ghost<bool>,
}

pub struct Heap<M: Mode> {
    core: Core,
    worker: u32,
    /// 解放の連鎖の作業リスト。使い回して、解放のたびに確保しない。
    work: Vec<Word>,
    /// `share` のたどりのスタック。使い回す。
    share_stack: Vec<*mut Header>,
    /// 検査モードの `share` が訪れた `lin` の物体。
    visited_lin: Vec<*mut Header>,
    /// 検査モードの token の表。項目は箱と、`into_token` のときの世代の番号である。
    tokens: Vec<Option<(*mut Header, u32)>>,
    dummy: Word,
    /// 検査モードで隔離した (使い回さない) ブロックのバイト数。
    quarantined: usize,
    stats: Stats,
    // `Heap` を `Sync` にしない印。Verus は `Cell` を受け付けないので、生のポインタで表す
    _mode: PhantomData<(M, *mut ())>,
}

/// ペイロードの語の数から、ヘッダを含む物体の語の数を出す。空いたブロックのペイロードの先頭に空きリストのポインタを
/// 置くので、ペイロードは少なくとも1語にする。
#[inline]
fn object_words(payload_words: usize) -> (r: usize)
    ensures
        r == HEADER_WORDS + if payload_words == 0 {
            1
        } else {
            payload_words as int
        },
{
    let payload = if payload_words == 0 {
        1
    } else {
        payload_words
    };
    // ふだんのビルドで桁あふれが回ると小さいブロックを取り、安全な `alloc_data` がその先まで書いてしまう
    // (docs/spec/object-model.md の「ページ」)
    match HEADER_WORDS.checked_add(payload) {
        Some(n) => n,
        None => too_large(),
    }
}

/// 解放の連鎖が子について返しうる誤り。モデルは不死の物体の外の参照の数、共有の物体の `lin` ビット、子の持ち主を
/// 持たないので、モデルが合っていても、検査モードは連鎖が手放す子にこれらを見つけうる (docs/spec/object-model.md
/// の「検査モードの約束」)。
pub open spec fn chain_err<T>(r: Result<T, HeapError>) -> bool {
    ||| r == Err::<T, HeapError>(HeapError::ImmortalOverRelease)
    ||| r == Err::<T, HeapError>(HeapError::LinShared)
    ||| r matches Err(HeapError::WrongOwner { .. })
}

/// `lo` 語目から `hi` 語目の手前まで、`()` を書いてある。
pub(crate) open spec fn unit_filled(p: BlockPerm, lo: int, hi: int) -> bool {
    forall|j: int|
        #![trigger p_init(p, j)]
        lo <= j < hi ==> p_init(p, j) && p_word(p, j).bits_spec() == 1
}

/// `()` を書いた範囲は、次の語に `()` を書くと1語伸びる。
proof fn lemma_unit_filled_extend(before: BlockPerm, after: BlockPerm, lo: int, hi: int)
    requires
        unit_filled(before, lo, hi),
        after.slots == before.slots.insert(hi, after.slots[hi]),
        after.slots[hi].is_init(),
        after.slots[hi].value().bits_spec() == 1,
    ensures
        unit_filled(after, lo, hi + 1),
{
    assert forall|j: int| #![trigger p_init(after, j)] lo <= j < hi + 1 implies p_init(after, j)
        && p_word(after, j).bits_spec() == 1 by {
        if j != hi {
            assert(p_init(before, j));
        }
    }
}

impl<M: Mode> Heap<M> {
    /// ヒープの不変条件。検査モードでは安全の条件 (`safe_wf`) で、速いモードではモデルが合っていること
    /// (`consistent`) である (docs/spec/object-model.md の「検査モードの約束」)。
    pub closed spec fn wf(&self) -> bool {
        &&& self.core.checked@ == M::CHECKED
        &&& self.tokens@ == self.core.model@.tokens
        &&& M::CHECKED ==> self.core.safe_wf() && self.dummy.is_ptr_spec() && self.core.word_ok(
            self.dummy,
        )
        &&& !M::CHECKED ==> self.consistent()
    }

    /// モデルが合っている。抽象のモデルとの対応 (`full`) が成り立ち、空ける途中の物体がなく、ダミーは生きている。
    pub closed spec fn consistent(&self) -> bool {
        &&& self.core.full()
        &&& self.core.quiet()
        &&& self.dummy_wf()
    }

    closed spec fn dummy_wf(&self) -> bool {
        let d = self.dummy.bits_spec();
        &&& self.dummy.is_ptr_spec()
        &&& self.core.word_ok(self.dummy)
        &&& self.core.state(d) == BlockState::Live
        &&& self.core.rc_of(d) == 0
        &&& self.core.kind(d) == Kind::BigInt
    }

    // API の前提の述語。ヒープの証明の部分 (`Core`) の述語を、そのまま見せる
    /// 構築中の物体がない。
    pub closed spec fn settled(&self) -> bool {
        self.core.model@.building is None
    }

    pub closed spec fn kind_of(&self, w: Word) -> Kind {
        self.core.kind(w.bits_spec())
    }

    /// 走査する語の数。
    pub closed spec fn count_of(&self, w: Word) -> int {
        self.core.count(w.bits_spec())
    }

    pub closed spec fn dummy_word(&self) -> Word {
        self.dummy
    }

    /// `old` の構築中の物体を生きている物体にし、rc と前置きの数の値だけを変えた (`dup`)。
    pub closed spec fn values_moved(&self, old: Heap<M>) -> bool {
        &&& self.core.values_changed(old.core.settled_view())
        &&& self.dummy == old.dummy
        &&& self.worker == old.worker
    }

    /// `dup` の前提がみな成り立ち、モデルが合っている。
    pub closed spec fn dup_ok(&self, w: Word) -> bool {
        &&& self.consistent()
        &&& self.settleable()
        &&& self.readable(w)
        &&& w.is_ptr_spec() ==> !self.lin_of(w)
        &&& self.owned(w)
    }

    /// `release` の前提がみな成り立ち、モデルが合っている。
    pub closed spec fn release_ok(&self, w: Word, tag: u16, keep: Seq<bool>) -> bool {
        &&& self.consistent()
        &&& self.settleable()
        &&& self.holds(w)
        &&& self.layout_of(w, tag, keep.len() as int)
        &&& self.rc_value(w) < 0 ==> !self.lin_of(w)
        &&& self.owned(w)
        &&& forall|i: int|
            #![trigger keep[i]]
            0 <= i < keep.len() && keep[i] ==> {
                let c = self.scanned_word(w, i);
                &&& c.is_ptr_spec() ==> !self.lin_of(c)
                &&& self.owned(c)
            }
    }

    /// `decref` の前提がみな成り立ち、モデルが合っている。
    pub closed spec fn decref_ok(&self, w: Word) -> bool {
        &&& self.consistent()
        &&& self.settleable()
        &&& self.holds(w)
        &&& w.is_ptr_spec() && self.rc_value(w) < 0 ==> !self.lin_of(w)
        &&& self.owned(w)
    }

    /// 構築中の物体を生きている物体にしただけのヒープ。`settle` は実行の時には何もしないので、物体を受ける操作が状態を
    /// 変える前に `Err` を返すと、ヒープはこれになる。
    pub closed spec fn settled_heap(&self) -> Heap<M> {
        Heap { core: self.core.settled_view(), ..*self }
    }

    /// token の表は変わらない。
    pub closed spec fn same_tokens(&self, old: Heap<M>) -> bool {
        &&& self.tokens == old.tokens
        &&& self.core.model@.tokens == old.core.model@.tokens
    }

    /// 物体とその数は変わらない。変わってよいのは、不死の物体の外に出ている参照の数 (docs/spec/object-model.md の
    /// 「不死の物体の数え方」) だけである。
    pub closed spec fn same_objects(&self, old: Heap<M>) -> bool {
        &&& self.core.recounted(old.core)
        &&& self.dummy == old.dummy
    }

    pub closed spec fn readable(&self, w: Word) -> bool {
        self.core.readable(w)
    }

    /// `w` の物体が生きているなら、局所の物体である (rc が正)。ほかのスレッドが読めない物体にだけ書くための安全の前提で、
    /// 構築中の物体と、生きていない物体 (検査モードが書く前に断る) もこれを満たす。
    pub closed spec fn unshared(&self, w: Word) -> bool {
        self.core.state(w.bits_spec()) == BlockState::Live ==> self.core.rc_of(w.bits_spec()) > 0
    }

    /// `w` は即値か、このヒープが確保したブロックを指す (安全の前提)。検査モードでは、空けたブロックも含む。
    pub closed spec fn word_in(&self, w: Word) -> bool {
        self.core.word_ok(w)
    }

    /// 構築中の物体はないか、書き終えている (安全の前提)。
    pub closed spec fn safe_settleable(&self) -> bool {
        self.core.safe_settleable()
    }

    /// モデルが合っていて、`w` が生きている物体で持ち主が今の worker なら、検査モードの生存と持ち主の検査は通る。
    proof fn lemma_checks_pass(&self, w: Word)
        requires
            self.wf(),
            w.is_ptr_spec(),
            self.word_in(w),
        ensures
            self.consistent() && self.settleable() && self.readable(w) ==> (self.core.state(
                w.bits_spec(),
            ) == BlockState::Live || self.core.state(w.bits_spec()) == BlockState::Building),
            self.consistent() && self.owned(w) && M::CHECKED && (self.core.state(w.bits_spec())
                == BlockState::Live || self.core.state(w.bits_spec()) == BlockState::Building)
                && self.core.rc_of(w.bits_spec()) > 0 ==> p_owner(self.core.perm(w.bits_spec()))
                == self.worker,
    {
        if self.consistent() {
            if self.settleable() && self.readable(w) {
                self.core.lemma_readable(w);
            }
            if M::CHECKED {
                self.core.lemma_code_state(w.bits_spec());
            }
        }
    }

    /// 構築中の物体の「残した」語を辺に数えた見方で、呼ぶ側が数えた参照を持つ。
    pub closed spec fn holds(&self, w: Word) -> bool {
        self.core.settled_view().holds(w)
    }

    pub closed spec fn settleable(&self) -> bool {
        self.core.settleable()
    }

    /// `alloc` と `alloc_immortal` の後の状態。`w` は構築中の物体である。
    pub closed spec fn built(
        &self,
        old: Heap<M>,
        w: Word,
        meta: Meta,
        rc: i32,
        payload_words: usize,
    ) -> bool {
        let h = w.bits_spec();
        &&& self.wf()
        &&& self.dummy == old.dummy
        &&& self.worker == old.worker
        &&& w.is_ptr_spec()
        &&& self.core.word_ok(w)
        &&& !old.core.dom().contains(h)
        &&& self.core.dom() == old.core.dom().insert(h)
        &&& self.core.state(h) == BlockState::Building
        &&& self.core.model@.building == Some(h)
        &&& self.core.model@.written[h] == Set::<int>::empty()
        &&& self.core.meta_bits(h) == meta.bits_spec()
        &&& self.core.rc_of(h) == rc
        &&& self.core.cap(h) >= payload_words
        &&& old.consistent() && old.settleable() ==> {
            &&& self.consistent()
            &&& self.core.frame(old.core)
            &&& rc != 0 ==> self.core.ext(h) == 1
        }
    }

    /// `alloc_data`、`alloc_int` の後の状態。`w` は生きている物体で、呼ぶ側がその参照を1つ持つ。
    pub closed spec fn made(&self, old: Heap<M>, w: Word) -> bool {
        let h = w.bits_spec();
        &&& self.wf()
        &&& self.dummy == old.dummy
        &&& self.worker == old.worker
        &&& self.core.model@.building is None
        &&& w.is_ptr_spec()
        &&& self.core.word_ok(w)
        &&& !old.core.dom().contains(h)
        &&& self.core.dom() == old.core.dom().insert(h)
        &&& self.core.state(h) == BlockState::Live
        &&& self.core.rc_of(h) == 1
        &&& old.consistent() && old.settleable() ==> {
            &&& self.consistent()
            &&& self.core.frame(old.core)
            &&& self.core.holds(w)
            &&& self.core.ext(h) == 1
        }
    }

    /// ダミーの物体と、検査モードの数を除いて、空のヒープ。
    pub fn new() -> (r: Heap<M>)
        ensures
            r.wf(),
            r.consistent(),
            r.settled(),
    {
        let mut heap = Heap {
            core: Core {
                pages: Pages::new(
                    if M::CHECKED {
                        PREFIX_WORDS
                    } else {
                        0
                    },
                ),
                perms: Tracked(Map::tracked_empty()),
                model: Ghost(
                    Model {
                        state: Map::empty(),
                        refs: Map::empty(),
                        edges: Map::empty(),
                        stuck: Map::empty(),
                        owner: Map::empty(),
                        written: Map::empty(),
                        kept: Set::empty(),
                        building: None,
                        tokens: Seq::empty(),
                    },
                ),
                checked: Ghost(M::CHECKED),
            },
            worker: 0,
            work: Vec::new(),
            share_stack: Vec::new(),
            visited_lin: Vec::new(),
            tokens: Vec::new(),
            dummy: Word::EMPTY,
            quarantined: 0,
            stats: Stats {
                rc_increments: 0,
                rc_decrements: 0,
                allocations: 0,
                live: 0,
                peak_live: 0,
            },
            _mode: PhantomData,
        };
        proof {
            heap.core.lemma_empty();
            heap.core.lemma_safe_empty();
        }
        // ダミーは値 0 の BigInt の不死の物体で、検査モードでも数えない (docs/spec/object-model.md の「ダミーの値」)
        let (h, Tracked(perm)) = heap.alloc_header(1, 0, Meta::new(0, 0, Kind::BigInt, false));
        let tracked mut perm = perm;
        let tracked mut slot = perm.slots.tracked_remove(0);
        ptr_mut_write(slot_ptr(h, 0), Tracked(&mut slot), Word::from_bits(0));
        proof {
            perm.slots.tracked_insert(0, slot);
        }
        if M::CHECKED {
            let tracked mut pre = perm.prefix.tracked_unwrap();
            ptr_mut_write(meta::prefix_first_ptr(h), Tracked(&mut pre.first), UNCOUNTED);
            proof {
                perm.prefix = Some(pre);
            }
        }
        proof {
            check::lemma_alive_code();
        }
        heap.core.publish(Ghost(h), Tracked(perm), Ghost(BlockState::Live), Ghost(0));
        heap.dummy = Word::from_ptr(h);
        proof {
            crate::word::lemma_aligned_is_ptr(h@.addr);
            assert(heap.core.word_ok(heap.dummy));
            assert(heap.dummy_wf());
        }
        heap
    }

    pub fn dummy(&self) -> (w: Word)
        ensures
            w == self.dummy_word(),
    {
        self.dummy
    }

    pub fn worker(&self) -> u32 {
        self.worker
    }

    /// 論理的な worker ID を替える。これから確保する物体の所有者と、所有者の検査の相手になる。
    pub fn set_worker(&mut self, worker: u32)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            old(self).consistent() ==> final(self).consistent(),
            final(self).same_objects(*old(self)),
    {
        self.worker = worker;
        proof {
            self.core.lemma_recounted_refl();
        }
    }

    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// 書き終えた構築中の物体を、生きている物体にする。実行の時には何もしない。
    #[inline(always)]
    fn settle(&mut self)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            !M::CHECKED ==> old(self).settleable(),
        ensures
            *final(self) == old(self).settled_heap(),
            final(self).core.model@.tokens == old(self).core.model@.tokens,
            final(self).tokens == old(self).tokens,
            final(self).wf(),
            old(self).consistent() && old(self).settleable() ==> final(self).consistent(),
            final(self).core.model@.building is None,
            final(self).core.frame(old(self).core),
            final(self).core == old(self).core.settled_view(),
            final(self).core.dom() == old(self).core.dom(),
            final(self).core.pages == old(self).core.pages,
            final(self).dummy == old(self).dummy,
            final(self).worker == old(self).worker,
            forall|w: Word| #[trigger] final(self).core.word_ok(w) == old(self).core.word_ok(w),
    {
        let ghost s0 = self.core;
        let ghost c = self.consistent() && self.settleable();
        proof {
            if c {
                reveal(Core::building_wf);
                assert(s0.state(self.dummy.bits_spec()) == BlockState::Live);
            }
        }
        self.core.settle();
        proof {
            if c {
                assert forall|b: usize| #[trigger]
                    self.core.dom().contains(b) implies self.core.state(b) != BlockState::Dying by {
                    assert(s0.dom().contains(b));
                }
            }
        }
    }

    // 確保
    /// ブロックを取り、ヘッダと (検査モードでは) 前置きを書く。ペイロードは書かない。ブロックの権限は呼ぶ側に渡し、呼ぶ側が
    /// ペイロードを書いてから `Core::publish` でヒープに入れる。
    #[inline]
    fn alloc_header(&mut self, payload_words: usize, rc: i32, meta: Meta) -> (r: (
        *mut Header,
        Tracked<BlockPerm>,
    ))
        requires
            !M::CHECKED ==> old(self).core.full(),
            M::CHECKED ==> old(self).core.safe_wf(),
            old(self).core.checked@ == M::CHECKED,
            meta.kind_spec() != Kind::Free,
        ensures
            final(self).core.model@.tokens == old(self).core.model@.tokens,
            final(self).tokens == old(self).tokens,
            old(self).core.full() ==> final(self).core.pending(r.0@.addr),
            M::CHECKED ==> final(self).core.safe_pending(r.0@.addr),
            final(self).core.perms == old(self).core.perms,
            final(self).core.model == old(self).core.model,
            final(self).core.checked == old(self).core.checked,
            final(self).dummy == old(self).dummy,
            final(self).worker == old(self).worker,
            r.0 == final(self).core.block(r.0@.addr),
            final(self).core.pages.carved_block(r.0@.addr),
            final(self).core.cap(r.0@.addr) >= if payload_words == 0 {
                1
            } else {
                payload_words as int
            },
            r.1@.wf(r.0, final(self).core.cap(r.0@.addr), M::CHECKED),
            r.1@.text() == Seq::<u8>::empty(),
            r.1@.rc.is_init() && r.1@.rc.value() == rc,
            r.1@.meta.is_init() && r.1@.meta.value() == meta.bits_spec(),
            bits_wf(meta.bits_spec()),
            bits_kind(meta.bits_spec()) == meta.kind_spec(),
            bits_scan(meta.bits_spec()) == meta.scan_spec(),
            M::CHECKED ==> r.1@.prefix is Some && r.1@.prefix->0.first.is_init()
                && r.1@.prefix->0.state.is_init() && r.1@.prefix->0.state.value() == ALIVE_STATE
                && (rc != 0 ==> r.1@.prefix->0.first.value() == old(self).worker),
            r.0@.addr % 8 == 0,
            r.0@.addr >= 8,
            r.0@.addr + 8 * (HEADER_WORDS + final(self).core.cap(r.0@.addr)) <= usize::MAX,
            forall|b: usize| #[trigger]
                old(self).core.dom().contains(b) ==> final(self).core.block(b) == old(
                    self,
                ).core.block(b) && final(self).core.cap(b) == old(self).core.cap(b),
            forall|w: Word| #[trigger] final(self).core.word_ok(w) == old(self).core.word_ok(w),
    {
        let ghost s0 = self.core;
        let words = object_words(payload_words);
        let (h, Tracked(perm)) = self.core.pages.alloc(words);
        proof {
            self.core.lemma_alloc(s0, words, h, perm);
            self.core.pages.lemma_block_bounds(h@.addr);
        }
        let tracked mut perm = perm;
        proof {
            // 確保器は前に使ったブロックの権限を渡すので、前の文字列のバイト列を捨てる (信頼の一覧の T10)
            perm.clear_text();
        }
        ptr_mut_write(rc_ptr(h), Tracked(&mut perm.rc), rc);
        ptr_mut_write(meta_ptr(h), Tracked(&mut perm.meta), meta.bits());
        if M::CHECKED {
            let tracked mut pre = perm.prefix.tracked_unwrap();
            init_prefix(
                h,
                if rc == 0 {
                    0
                } else {
                    self.worker
                },
                Tracked(&mut pre),
            );
            proof {
                perm.prefix = Some(pre);
            }
            self.stats.allocations = self.stats.allocations.wrapping_add(1);
            self.stats.live = self.stats.live.wrapping_add(1);
            if self.stats.live > self.stats.peak_live {
                self.stats.peak_live = self.stats.live;
            }
        }
        (h, Tracked(perm))
    }

    /// 物体を確保する。ペイロードは呼ぶ側が書く。
    ///
    /// # Safety
    /// `meta` の種類は `Foreign`、`String`、`Free` のどれでもなく、`scan` は 255 でなく、走査する語はペイロードに収まる。呼ぶ側は、
    /// ほかのヒープの操作の前に、`meta` に合うペイロード (走査する語、`BigInt` なら値) を書く。
    #[inline]
    pub unsafe fn alloc(&mut self, payload_words: usize, meta: Meta) -> (w: Word)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            !M::CHECKED ==> old(self).settleable(),
            meta.kind_spec() != Kind::Foreign && meta.kind_spec() != Kind::String
                && meta.kind_spec() != Kind::Free,
            meta.scan_spec() != 255,
            meta.scan_spec() <= payload_words,
        ensures
            final(self).built(*old(self), w, meta, 1, payload_words),
    {
        self.alloc_building(payload_words, 1, meta)
    }

    /// 不死の物体を確保する。検査モードでは、外に出ている参照の数を 0 にして作る。
    ///
    /// # Safety
    /// `alloc` と同じ。
    pub unsafe fn alloc_immortal(&mut self, payload_words: usize, meta: Meta) -> (w: Word)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            !M::CHECKED ==> old(self).settleable(),
            meta.kind_spec() != Kind::Foreign && meta.kind_spec() != Kind::String
                && meta.kind_spec() != Kind::Free,
            meta.scan_spec() != 255,
            meta.scan_spec() <= payload_words,
        ensures
            final(self).built(*old(self), w, meta, 0, payload_words),
    {
        self.alloc_building(payload_words, 0, meta)
    }

    #[inline]
    fn alloc_building(&mut self, payload_words: usize, rc: i32, meta: Meta) -> (w: Word)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            !M::CHECKED ==> old(self).settleable(),
            rc == 0 || rc == 1,
            meta.kind_spec() != Kind::Foreign && meta.kind_spec() != Kind::String
                && meta.kind_spec() != Kind::Free,
            meta.scan_spec() != 255,
            meta.scan_spec() <= payload_words,
        ensures
            final(self).built(*old(self), w, meta, rc, payload_words),
    {
        debug_assert!(!matches!(meta.kind(), Kind::Foreign | Kind::String | Kind::Free));
        self.settle();
        let ghost s1 = *self;
        let (h, Tracked(perm)) = self.alloc_header(payload_words, rc, meta);
        let ghost mid = self.core;
        proof {
            check::lemma_alive_code();
        }
        self.core.publish(Ghost(h), Tracked(perm), Ghost(BlockState::Building), Ghost(self.worker));
        let w = Word::from_ptr(h);
        proof {
            if old(self).consistent() && old(self).settleable() {
                self.lemma_dummy_kept(s1);
                self.core.lemma_frame_chain(mid, s1.core, old(self).core);
            } else {
                self.lemma_published_safe(s1);
            }
            crate::word::lemma_aligned_is_ptr(h@.addr);
        }
        w
    }

    /// 検査モードで新しいブロックを入れた後も、安全の条件とダミーの語は保たれる。
    proof fn lemma_published_safe(&self, s1: Heap<M>)
        requires
            M::CHECKED,
            s1.wf(),
            self.core.safe_wf(),
            self.tokens@ == self.core.model@.tokens,
            self.core.checked == s1.core.checked,
            self.dummy == s1.dummy,
            forall|w: Word| s1.core.word_ok(w) ==> #[trigger] self.core.word_ok(w),
        ensures
            self.wf(),
    {
    }

    /// 検査モードで、ブロックの集まりと確保器を変えない操作の後も、安全の条件とダミーの語は保たれる。
    proof fn lemma_safe_frame(&self, s1: Heap<M>)
        requires
            M::CHECKED,
            s1.wf(),
            self.core.safe_wf(),
            self.tokens@ == self.core.model@.tokens,
            self.core.checked == s1.core.checked,
            self.dummy == s1.dummy,
            self.core.dom() == s1.core.dom(),
            self.core.pages == s1.core.pages,
        ensures
            self.wf(),
    {
    }

    /// 新しいブロックを入れた後も、ダミーは変わらない。
    proof fn lemma_dummy_kept(&self, s1: Heap<M>)
        requires
            self.tokens@ == self.core.model@.tokens,
            s1.wf(),
            s1.consistent(),
            self.core.full(),
            M::CHECKED ==> self.core.safe_wf(),
            self.core.checked == s1.core.checked,
            self.dummy == s1.dummy,
            self.core.frame(s1.core),
            s1.core.model@.building is None,
            forall|w: Word| s1.core.word_ok(w) ==> #[trigger] self.core.word_ok(w),
            forall|b: usize| #[trigger]
                self.core.dom().contains(b) && !s1.core.dom().contains(b) ==> self.core.state(b)
                    != BlockState::Dying,
        ensures
            self.wf(),
            self.consistent(),
    {
        assert(s1.core.word_ok(s1.dummy));
        assert(s1.core.dom().contains(s1.dummy.bits_spec()));
        assert forall|b: usize| #[trigger] self.core.dom().contains(b) implies self.core.state(b)
            != BlockState::Dying by {
            if s1.core.dom().contains(b) {
                assert(s1.core.state(b) != BlockState::Dying);
            }
        }
    }

    /// データの物体を作る。走査する語が 255 個以上なら、ペイロードの先頭に数の語を置く。走査する語は、書く前に
    /// 解放の連鎖や `share` が読んでも子をたどらないように、`()` で埋めておく。
    pub fn alloc_data(&mut self, tag: u16, scanned: usize, raw: usize, lin: bool) -> (w: Word)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            !M::CHECKED ==> old(self).settleable(),
        ensures
            final(self).made(*old(self), w),
            final(self).kind_of(w) == Kind::Data,
            final(self).count_of(w) == scanned,
    {
        let extended = scanned >= Meta::EXTENDED_SCAN as usize;
        let scan = if extended {
            Meta::EXTENDED_SCAN
        } else {
            scanned as u8
        };
        let first: usize = if extended {
            1
        } else {
            0
        };
        let payload_words = match first.checked_add(scanned) {
            Some(n) => match n.checked_add(raw) {
                Some(m) => m,
                None => too_large(),
            },
            None => too_large(),
        };
        self.settle();
        let ghost s1 = *self;
        let meta = Meta::new(tag, scan, Kind::Data, lin);
        let (h, Tracked(perm)) = self.alloc_header(payload_words, 1, meta);
        let tracked mut perm = perm;
        if extended {
            let tracked mut slot = perm.slots.tracked_remove(0);
            ptr_mut_write(slot_ptr(h, 0), Tracked(&mut slot), Word::from_bits(scanned));
            proof {
                perm.slots.tracked_insert(0, slot);
            }
        }
        let ghost cap = self.core.cap(h@.addr);
        let mut i: usize = 0;
        while i < scanned
            invariant
                perm.wf(h, cap, M::CHECKED),
                perm.rc.is_init() && perm.rc.value() == 1,
                perm.meta.is_init() && perm.meta.value() == meta.bits_spec(),
                h@.addr + 8 * (HEADER_WORDS + cap) <= usize::MAX,
                first + scanned <= cap,
                first == if extended {
                    1usize
                } else {
                    0usize
                },
                extended ==> p_init(perm, 0) && p_word(perm, 0).bits_spec() == scanned,
                unit_filled(perm, first as int, first + i),
                0 <= i <= scanned,
            decreases scanned - i,
        {
            proof {
                if i > 0 {
                    assert(p_init(perm, (first + i - 1) as int));
                    assert(p_word(perm, (first + i - 1) as int).bits_spec() == 1);
                }
            }
            let unit = Word::UNIT;
            proof {
                assert(unit.bits_spec() == 1);
            }
            let ghost before = perm;

            let tracked mut slot = perm.slots.tracked_remove((first + i) as int);
            ptr_mut_write(slot_ptr(h, first + i), Tracked(&mut slot), unit);
            proof {
                assert(slot.is_init());
                assert(slot.value() == unit);
                perm.slots.tracked_insert((first + i) as int, slot);
                assert(perm.slots[(first + i) as int] == slot);
                assert(p_word(perm, (first + i) as int) == unit);
                assert(p_word(perm, (first + i) as int).bits_spec() == 1);
                assert(perm.slots =~= before.slots.insert((first + i) as int, slot));
                lemma_unit_filled_extend(before, perm, first as int, first + i);
                if extended {
                    assert(perm.slots[0] == before.slots[0]);
                }
            }
            i += 1;
        }
        let ghost mid = self.core;
        proof {
            assert(i == scanned);
            self.lemma_data_perm(perm, h, meta, first, scanned, extended);
        }
        self.core.publish(Ghost(h), Tracked(perm), Ghost(BlockState::Live), Ghost(self.worker));
        let w = Word::from_ptr(h);
        proof {
            if old(self).consistent() && old(self).settleable() {
                self.lemma_dummy_kept(s1);
                self.core.lemma_frame_chain(mid, s1.core, old(self).core);
            } else {
                self.lemma_published_safe(s1);
            }
            crate::word::lemma_aligned_is_ptr(h@.addr);
        }
        w
    }

    /// `alloc_data` が書いたブロックは、生きている物体として正しい。
    proof fn lemma_data_perm(
        &self,
        perm: BlockPerm,
        h: *mut Header,
        meta: Meta,
        first: usize,
        scanned: usize,
        extended: bool,
    )
        requires
            h == self.core.block(h@.addr),
            perm.wf(h, self.core.cap(h@.addr), self.core.checked@),
            perm.prefix matches Some(q) ==> q.first.is_init() && q.state.is_init()
                && q.state.value() == ALIVE_STATE && q.first.value() == self.worker,
            perm.rc.is_init(),
            perm.rc.value() == 1,
            perm.meta.is_init() && perm.meta.value() == meta.bits_spec(),
            meta.kind_spec() == Kind::Data,
            bits_wf(meta.bits_spec()),
            bits_kind(meta.bits_spec()) == meta.kind_spec(),
            bits_scan(meta.bits_spec()) == meta.scan_spec(),
            extended == (scanned >= 255),
            meta.scan_spec() == if extended {
                255u8
            } else {
                scanned as u8
            },
            first == if extended {
                1usize
            } else {
                0usize
            },
            first + scanned <= self.core.cap(h@.addr),
            extended ==> p_init(perm, 0) && p_word(perm, 0).bits_spec() == scanned,
            unit_filled(perm, first as int, first + scanned),
        ensures
            self.core.perm_wf(perm, h@.addr, BlockState::Live, Set::empty(), self.worker),
            p_count(perm) == scanned,
    {
        let bits = meta.bits_spec();
        check::lemma_alive_code();
        assert(bits_kind(bits) == Kind::Data);
        assert(bits_scan(bits) == meta.scan_spec());
        assert(p_count(perm) == scanned);
        assert(p_first(perm) == first);
        assert forall|i: int| #[trigger] p_scanned(perm, i) implies self.core.slot_word(
            perm,
            h@.addr,
            i,
        ) by {
            assert(first <= i < first + scanned);
            assert(p_init(perm, i));
            assert(p_word(perm, i).bits_spec() == 1);
            crate::word::lemma_unit_bits();
        }
    }

    /// `Int` を `tobj` に入れる。63 ビットに収まれば即値、収まらなければ `BigInt` の物体にする。
    pub fn alloc_int(&mut self, n: i64) -> (w: Word)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            !M::CHECKED ==> old(self).settleable(),
        ensures
            w.is_imm_spec() ==> *final(self) == *old(self),
            !w.is_imm_spec() ==> final(self).made(*old(self), w) && final(self).kind_of(w)
                == Kind::BigInt,
    {
        match Word::int(n) {
            Some(w) => {
                proof {
                    crate::word::lemma_imm_round_trip(n);
                }
                w
            },
            None => {
                self.settle();
                let ghost s1 = *self;
                let (h, Tracked(perm)) = self.alloc_header(
                    1,
                    1,
                    Meta::new(0, 0, Kind::BigInt, false),
                );
                let tracked mut perm = perm;
                let tracked mut slot = perm.slots.tracked_remove(0);
                ptr_mut_write(slot_ptr(h, 0), Tracked(&mut slot), Word::from_bits(n as usize));
                proof {
                    perm.slots.tracked_insert(0, slot);
                }
                let ghost mid = self.core;
                proof {
                    check::lemma_alive_code();
                }
                self.core.publish(
                    Ghost(h),
                    Tracked(perm),
                    Ghost(BlockState::Live),
                    Ghost(self.worker),
                );
                let w = Word::from_ptr(h);
                proof {
                    if old(self).consistent() && old(self).settleable() {
                        self.lemma_dummy_kept(s1);
                        self.core.lemma_frame_chain(mid, s1.core, old(self).core);
                    } else {
                        self.lemma_published_safe(s1);
                    }
                    crate::word::lemma_aligned_is_ptr(w.bits_spec());
                }
                w
            },
        }
    }
}

} // verus!

/// 語の数の計算が桁あふれしたときの panic の文言。
pub(crate) const TOO_LARGE: &str = "an object of more words than the address space holds";

impl<M: Mode> Default for Heap<M> {
    fn default() -> Heap<M> {
        Heap::new()
    }
}

// `Heap` は worker ごとに1つ持ち、worker のスレッドに移せる。`Sync` にはしない (docs/spec/object-model.md の「語」)
unsafe impl<M: Mode> Send for Heap<M> {}

verus! {

impl<M: Mode> Heap<M> {
    pub closed spec fn lin_of(&self, w: Word) -> bool {
        self.core.lin(w.bits_spec())
    }

    pub closed spec fn rc_value(&self, w: Word) -> i32 {
        self.core.rc_of(w.bits_spec())
    }

    /// rc が正の物体の持ち主は、今の worker である。
    pub closed spec fn owned(&self, w: Word) -> bool {
        w.is_ptr_spec() && self.core.rc_of(w.bits_spec()) > 0
            ==> self.core.model@.owner[w.bits_spec()] == self.worker
    }

    /// `old` の生きている物体のうち、不死か外の参照を持ち続けるものは、同じ中身で生きている。外の参照の数は `w` だけ
    /// `delta` 変わる。
    pub closed spec fn evolved(&self, old: Heap<M>, w: Word, delta: int) -> bool {
        &&& self.dummy == old.dummy
        &&& self.worker == old.worker
        &&& self.core.evolved(
            old.core.settled_view(),
            w.bits_spec(),
            if w.is_ptr_spec() {
                delta
            } else {
                0
            },
        )
    }

    /// `w` の物体はデータで、tag が `tag`、走査する語の数が `n` である。
    pub closed spec fn layout_of(&self, w: Word, tag: u16, n: int) -> bool {
        let h = w.bits_spec();
        &&& self.core.kind(h) == Kind::Data
        &&& bits_tag(self.core.meta_bits(h)) == tag
        &&& self.core.count(h) == n
    }

    /// `w` の参照を手放し、残すフィールドの参照を1つずつ呼ぶ側に渡した後の状態。
    pub closed spec fn handed_over(&self, old: Heap<M>, w: Word, keep: Seq<bool>) -> bool {
        let v = old.core.settled_view();
        &&& self.dummy == old.dummy
        &&& self.worker == old.worker
        &&& self.core.evolved_by(v, handover(v.kept_out(w.bits_spec(), keep), w.bits_spec()))
    }

    /// `dup` の後の状態。`w` の物体の rc と数だけが動く。
    pub closed spec fn bumped(&self, old: Heap<M>, w: Word) -> bool {
        &&& self.dummy == old.dummy
        &&& self.worker == old.worker
        &&& self.tokens == old.tokens
        &&& self.core.bumped(old.core.settled_view(), w.bits_spec())
    }

    /// ダミーの物体は不死なので、`evolved` の後も同じ形で生きている。
    proof fn lemma_dummy_evolved(&self, old: Heap<M>, w: usize, delta: int)
        requires
            old.dummy_wf(),
            self.dummy == old.dummy,
            self.core.evolved(old.core.settled_view(), w, delta),
        ensures
            self.dummy_wf(),
    {
        reveal(Core::building_wf);
        let v = old.core.settled_view();
        let d = old.dummy.bits_spec();
        assert(old.core.dom().contains(d));
        assert(v.dom().contains(d));
        assert(v.state(d) == BlockState::Live);
        assert(v.rc_of(d) == 0);
    }

    /// ダミーの物体は不死なので、`evolved_by` の後も同じ形で生きている。
    proof fn lemma_dummy_by(&self, old: Heap<M>, d: spec_fn(usize) -> int)
        requires
            old.dummy_wf(),
            self.dummy == old.dummy,
            self.core.evolved_by(old.core.settled_view(), d),
        ensures
            self.dummy_wf(),
    {
        reveal(Core::building_wf);
        let v = old.core.settled_view();
        let dd = old.dummy.bits_spec();
        assert(old.core.dom().contains(dd));
        assert(v.dom().contains(dd));
        assert(v.state(dd) == BlockState::Live);
        assert(v.settled_state(dd) == BlockState::Live);
        assert(v.rc_of(dd) == 0);
    }

    /// 構築中の物体を生きている物体にしただけなら、`evolved` を満たす。
    proof fn lemma_evolved_refl(&self, old: Heap<M>, w: Word, delta: int)
        requires
            self.core.frame(old.core),
            self.core.model@.building is None,
            self.dummy == old.dummy,
            self.worker == old.worker,
            !w.is_ptr_spec(),
        ensures
            self.evolved(old, w, delta),
    {
        self.core.lemma_evolved_self(w.bits_spec());
        self.core.lemma_frame_evolved(self.core, old.core, w.bits_spec(), 0);
    }

    /// `settle` の後の `mid` から `evolved` なら、`settle` の前からも `evolved` である。
    proof fn lemma_evolved_settled(&self, old: Heap<M>, mid: Heap<M>, w: Word, delta: int)
        requires
            mid.core.frame(old.core),
            mid.core.model@.building is None,
            mid.dummy == old.dummy,
            mid.worker == old.worker,
            self.evolved(mid, w, delta),
        ensures
            self.evolved(old, w, delta),
    {
        self.core.lemma_frame_evolved(
            mid.core,
            old.core,
            w.bits_spec(),
            if w.is_ptr_spec() {
                delta
            } else {
                0
            },
        );
    }

    /// 数を1つ動かした後。`s0` は `settle` の後の状態である。
    proof fn lemma_after_step(&self, old: Heap<M>, s0: Core, h: usize, w: Word, delta: int)
        requires
            self.tokens@ == self.core.model@.tokens,
            old.dummy_wf(),
            s0.frame(old.core),
            self.dummy == old.dummy,
            self.worker == old.worker,
            self.core.full(),
            M::CHECKED ==> self.core.safe_wf(),
            self.core.checked@ == M::CHECKED,
            self.core.quiet(),
            self.core.evolved(s0, h, delta),
            w.is_ptr_spec(),
            w.bits_spec() == h,
        ensures
            self.wf(),
            self.consistent(),
            self.evolved(old, w, delta),
    {
        self.core.lemma_frame_evolved(s0, old.core, h, delta);
        self.lemma_dummy_evolved(old, h, delta);
    }

    /// 不死の物体の数を替えても、ヒープの不変条件は保たれる。
    proof fn lemma_recounted_heap(&self, old: Heap<M>)
        requires
            self.tokens@ == self.core.model@.tokens,
            old.wf(),
            self.core.recounted(old.core),
            self.dummy == old.dummy,
            M::CHECKED,
        ensures
            self.wf(),
            old.consistent() ==> self.consistent(),
    {
        if old.consistent() {
            self.core.lemma_recounted(old.core);
            assert(old.core.dom().contains(self.dummy.bits_spec()));
        }
        self.core.lemma_recounted_values(old.core);
        self.core.lemma_safe_values(old.core);
    }

    /// `dup` の後に不死の物体 `w` の数を増やしても、`dup` の後の状態のままである。
    proof fn lemma_recounted_dup(&self, mid: Heap<M>, old: Heap<M>, w: Word)
        requires
            M::CHECKED,
            self.tokens == mid.tokens,
            self.tokens@ == self.core.model@.tokens,
            mid.wf(),
            mid.consistent(),
            mid.evolved(old, w, 1),
            mid.bumped(old, w),
            old.core.model@.building is None,
            old.core.dom().contains(w.bits_spec()),
            old.core.rc_of(w.bits_spec()) == 0,
            w.is_ptr_spec(),
            self.core.recounted(mid.core),
            forall|b: usize|
                b != w.bits_spec() ==> #[trigger] self.core.perms@[b] == mid.core.perms@[b],
            self.dummy == mid.dummy,
            self.worker == mid.worker,
        ensures
            self.wf(),
            self.consistent(),
            self.evolved(old, w, 1),
            self.bumped(old, w),
    {
        let h = w.bits_spec();
        self.lemma_recounted_heap(mid);
        self.core.lemma_recounted_evolved(mid.core, old.core.settled_view(), h, 1);
        assert(mid.core.dom().contains(h));
    }

    /// 最後の参照を手放して解放の連鎖を走らせた後。`s1` は `h` を空ける途中にした状態である。
    proof fn lemma_after_chain(&self, old: Heap<M>, s0: Core, s1: Core, h: usize, w: Word)
        requires
            self.tokens@ == self.core.model@.tokens,
            old.dummy_wf(),
            s0.frame(old.core),
            s1.evolved(s0, h, -1),
            self.dummy == old.dummy,
            self.worker == old.worker,
            self.core.full(),
            M::CHECKED ==> self.core.safe_wf(),
            self.core.checked@ == M::CHECKED,
            self.core.quiet(),
            self.core.evolved(s1, 0, 0),
            w.is_ptr_spec(),
            w.bits_spec() == h,
        ensures
            self.wf(),
            self.consistent(),
            self.evolved(old, w, -1),
    {
        self.core.lemma_evolved_trans(s1, s0, h, -1, 0);
        self.lemma_after_step(old, s0, h, w, -1);
    }

    /// 語 `w` のヘッダ。`w` はこのヒープのブロックを指す。
    #[inline]
    fn header_of(&self, w: Word) -> (h: *mut Header)
        requires
            self.core.sound(),
            self.core.word_ok(w),
            w.is_ptr_spec(),
        ensures
            h@.addr == w.bits_spec(),
            h == self.core.block(h@.addr),
    {
        let h = w.as_ptr();
        proof {
            self.core.lemma_shape(h@.addr);
            vstd::raw_ptr::ptrs_mut_eq_sized(h);
            vstd::raw_ptr::ptrs_mut_eq_sized(self.core.block(h@.addr));
        }
        h
    }

    /// # Safety
    /// `w` は即値か、このヒープの生きている物体を指し、呼ぶ側が参照を持っているか、生きている持ち主から借りている。
    /// 物体は `lin` ビットを持たない。rc が正なら、持ち主は今の worker である。
    #[inline]
    pub unsafe fn dup(&mut self, w: Word) -> (r: Result<(), HeapError>)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            old(self).word_in(w),
            !M::CHECKED ==> old(self).dup_ok(w),
        ensures
            final(self).wf(),
            final(self).values_moved(*old(self)),
            final(self).same_tokens(*old(self)),
            r is Err ==> *final(self) == old(self).settled_heap(),
            !M::CHECKED ==> r is Ok,
            old(self).dup_ok(w) ==> r is Ok && final(self).consistent() && final(self).evolved(
                *old(self),
                w,
                1,
            ) && final(self).bumped(*old(self), w),
    {
        let ghost c = self.dup_ok(w);
        self.settle();
        if !w.is_ptr() {
            proof {
                self.core.lemma_values_refl();
                if c {
                    self.lemma_evolved_refl(*old(self), w, 1);
                    if self.core.dom().contains(w.bits_spec()) {
                        self.core.lemma_at(w.bits_spec());
                        crate::word::lemma_aligned_is_ptr(w.bits_spec());
                    }
                }
            }
            return Ok(());
        }
        let h = self.header_of(w);
        // 局所の速い道だけを呼ぶ側に展開し、`Result` の分岐を消す (docs/spec/object-model.md の「核の操作」)。検査モードは
        // rc より先に前置きを読むので、いつも遅い道に進む
        if !M::CHECKED {
            let n = self.core.rc_get(h);
            if let DupStep::Store(m) = rc::dup_step(n) {
                let ghost s0 = self.core;
                proof {
                    s0.lemma_at(h@.addr);
                }
                self.core.rc_put(h, m);
                let ghost mid = self.core;
                self.core.count_ghost(
                    Ghost(s0),
                    Ghost(h@.addr),
                    Ghost(true),
                    Ghost(None),
                    Ghost(true),
                    Ghost(false),
                );
                proof {
                    self.lemma_after_step(*old(self), s0, h@.addr, w, 1);
                    self.core.lemma_values_rc(mid, s0, h@.addr);
                }
                return Ok(());
            }
        }
        let ghost mid = *self;
        proof {
            if c {
                self.core.lemma_readable(w);
            }
        }
        let r = self.dup_slow(h, Ghost(w), Ghost(c));
        proof {
            if c {
                self.lemma_evolved_settled(*old(self), mid, w, 1);
            }
        }
        r
    }

    /// `dup` の遅い道。共有、不死、張り付いた数と、検査モードの仕事を受け持つ。
    #[cold]
    #[inline(never)]
    fn dup_slow(&mut self, h: *mut Header, Ghost(w): Ghost<Word>, Ghost(c): Ghost<bool>) -> (r:
        Result<(), HeapError>)
        requires
            old(self).word_in(w),
            w.is_ptr_spec() && w.bits_spec() == h@.addr,
            old(self).wf(),
            old(self).core.model@.building is None,
            old(self).core.dom().contains(h@.addr),
            h == old(self).core.block(h@.addr),
            !M::CHECKED ==> c,
            c ==> old(self).consistent() && old(self).core.state(h@.addr) == BlockState::Live
                && !old(self).core.lin(h@.addr) && old(self).owned(w),
        ensures
            r is Err ==> *final(self) == *old(self),
            final(self).core.values_changed(old(self).core),
            final(self).dummy == old(self).dummy,
            final(self).worker == old(self).worker,
            final(self).wf(),
            final(self).same_tokens(*old(self)),
            !M::CHECKED ==> r is Ok,
            c ==> r is Ok && final(self).consistent() && final(self).evolved(*old(self), w, 1)
                && final(self).bumped(*old(self), w),
    {
        proof {
            self.lemma_checks_pass(w);
            self.core.lemma_values_refl();
        }
        let counted = self.checked_dup(h)?;
        let ghost s0 = self.core;
        proof {
            s0.lemma_shape(h@.addr);
            if c {
                s0.lemma_at(h@.addr);
                s0.lemma_frame_refl();
            }
            if M::CHECKED {
                reveal(Core::building_wf);
                assert(s0.state(h@.addr) == BlockState::Live);
            }
        }
        let n = self.core.rc_get(h);
        match rc::dup_step(n) {
            DupStep::Store(m) => self.core.rc_put(h, m),
            DupStep::Shared => self.core.rc_up_shared(h),
            DupStep::Skip => {},
        }
        let ghost mid = self.core;
        self.core.count_ghost(
            Ghost(s0),
            Ghost(h@.addr),
            Ghost(true),
            Ghost(None),
            Ghost(c),
            Ghost(false),
        );
        proof {
            self.core.lemma_values_rc(mid, s0, h@.addr);
        }
        if counted {
            self.stats.rc_increments = self.stats.rc_increments.wrapping_add(1);
        }
        proof {
            if c {
                self.lemma_after_step(*old(self), s0, h@.addr, w, 1);
            } else {
                self.lemma_published_safe(*old(self));
            }
        }
        if M::CHECKED && n == 0 && counted {
            let ghost mid = *self;
            self.count_immortal(h, true);
            proof {
                self.core.lemma_recounted_values(mid.core);
                self.core.lemma_values_trans(mid.core, s0);
                self.lemma_recounted_heap(mid);
                if c {
                    self.lemma_recounted_dup(mid, *old(self), w);
                }
            }
        }
        Ok(())
    }

    /// 参照を1つ手放す。LIR の `decref` と `drop` は、どちらもこれで実行する (docs/spec/object-model.md の「核の操作」)。
    ///
    /// # Safety
    /// `w` は即値か、このヒープの生きている物体を指し、呼ぶ側がその参照を1つ持っている。不死の物体なら、その参照は
    /// `acquire_immortal` か `dup` で作ったものである。共有の物体は `lin` ビットを持たない。rc が正なら、持ち主は今の worker である。
    #[inline]
    pub unsafe fn decref(&mut self, w: Word) -> (r: Result<(), HeapError>)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            old(self).word_in(w),
            !M::CHECKED ==> old(self).decref_ok(w),
        ensures
            final(self).wf(),
            final(self).same_tokens(*old(self)),
            !M::CHECKED ==> r is Ok,
            old(self).decref_ok(w) ==> r is Ok || chain_err(r),
            old(self).decref_ok(w) && r is Ok ==> final(self).consistent() && final(self).evolved(
                *old(self),
                w,
                -1,
            ),
    {
        let ghost c = self.decref_ok(w);
        self.settle();
        if !w.is_ptr() {
            proof {
                if c {
                    self.lemma_evolved_refl(*old(self), w, -1);
                }
            }
            return Ok(());
        }
        let h = self.header_of(w);
        // `dup` と同じく、局所の数を減らすだけの道を呼ぶ側に展開する。一意な物体を空けることは VM でよく起きるので、
        // 遅い道に入れず、ここから解放の連鎖を直に呼ぶ (docs/implementation/benchmarks.md の「S9a (`eml_rt` の核)」)
        if !M::CHECKED {
            let n = self.core.rc_get(h);
            match rc::decref_step(n) {
                DecrefStep::Store(m) => {
                    let ghost s0 = self.core;
                    proof {
                        s0.lemma_at(h@.addr);
                    }
                    self.core.rc_put(h, m);
                    self.core.count_ghost(
                        Ghost(s0),
                        Ghost(h@.addr),
                        Ghost(false),
                        Ghost(None),
                        Ghost(true),
                        Ghost(s0.last_by_model(h@.addr, false)),
                    );
                    proof {
                        self.lemma_after_step(*old(self), s0, h@.addr, w, -1);
                    }
                    return Ok(());
                },
                DecrefStep::Free => {
                    let ghost s0 = self.core;
                    proof {
                        s0.lemma_at(h@.addr);
                    }
                    self.core.count_ghost(
                        Ghost(s0),
                        Ghost(h@.addr),
                        Ghost(false),
                        Ghost(None),
                        Ghost(true),
                        Ghost(s0.last_by_model(h@.addr, false)),
                    );
                    let ghost s1 = self.core;
                    let r = self.free_chain(h, Ghost(true));
                    proof {
                        if r is Ok {
                            self.lemma_after_chain(*old(self), s0, s1, h@.addr, w);
                        }
                    }
                    return r;
                },
                DecrefStep::Shared | DecrefStep::Skip => {},
            }
        }
        let ghost mid = *self;
        proof {
            if c {
                self.core.lemma_readable(w);
            }
        }
        let r = self.decref_slow(h, Ghost(w), Ghost(c));
        proof {
            if c && r is Ok {
                self.lemma_evolved_settled(*old(self), mid, w, -1);
            }
        }
        r
    }

    /// `decref` の遅い道。共有、不死、張り付いた数と、検査モードの仕事 (検査モードの解放の連鎖を含む) を受け持つ。
    #[cold]
    #[inline(never)]
    fn decref_slow(&mut self, h: *mut Header, Ghost(w): Ghost<Word>, Ghost(c): Ghost<bool>) -> (r:
        Result<(), HeapError>)
        requires
            w.is_ptr_spec() && w.bits_spec() == h@.addr,
            old(self).wf(),
            old(self).core.model@.building is None,
            old(self).core.dom().contains(h@.addr),
            h == old(self).core.block(h@.addr),
            !M::CHECKED ==> c,
            c ==> old(self).consistent() && old(self).core.state(h@.addr) == BlockState::Live && (
            old(self).core.rc_of(h@.addr) != 0 ==> old(self).core.ext(h@.addr) >= 1),
        ensures
            final(self).wf(),
            final(self).same_tokens(*old(self)),
            !M::CHECKED ==> r is Ok,
            c ==> r is Ok || chain_err(r),
            c && r is Ok ==> final(self).consistent() && final(self).evolved(*old(self), w, -1),
    {
        let ghost s0 = self.core;
        proof {
            if c {
                s0.lemma_frame_refl();
            }
        }
        let last = self.release_ref(h, Ghost(None), Ghost(c))?;
        if last {
            let ghost s1 = self.core;
            let r = self.free_chain(h, Ghost(c));
            proof {
                if c && r is Ok {
                    self.lemma_after_chain(*old(self), s0, s1, h@.addr, w);
                }
                if M::CHECKED {
                    self.lemma_safe_frame(*old(self));
                }
            }
            r
        } else {
            proof {
                if c {
                    self.lemma_after_step(*old(self), s0, h@.addr, w, -1);
                }
                if M::CHECKED {
                    self.lemma_safe_frame(*old(self));
                }
            }
            Ok(())
        }
    }

    /// 参照を1つ手放し、最後の参照なら真を返す。箱はまだ空けない。`edge` は、解放の連鎖が手放す親の辺である。`c` は、
    /// モデルが合っていてモデルの前提を守った道である。
    fn release_ref(
        &mut self,
        h: *mut Header,
        Ghost(edge): Ghost<Option<Pos>>,
        Ghost(c): Ghost<bool>,
    ) -> (r: Result<bool, HeapError>)
        requires
            !M::CHECKED ==> c,
            c ==> old(self).core.full(),
            M::CHECKED ==> old(self).core.safe_wf(),
            old(self).core.checked@ == M::CHECKED,
            old(self).core.dom().contains(h@.addr),
            h == old(self).core.block(h@.addr),
            old(self).core.model@.building is None,
            c ==> old(self).core.state(h@.addr) == BlockState::Live,
            c ==> (edge matches Some(e) ==> old(self).core.model@.edges.dom().contains(e) && old(
                self,
            ).core.model@.edges[e] == h@.addr && old(self).core.state(e.0) == BlockState::Dying),
            c ==> (edge is None && old(self).core.rc_of(h@.addr) != 0 ==> old(self).core.ext(
                h@.addr,
            ) >= 1),
        ensures
            c ==> r is Ok || chain_err(r),
            final(self).core.pages == old(self).core.pages,
            final(self).core.model@.tokens == old(self).core.model@.tokens,
            final(self).tokens == old(self).tokens,
            c ==> final(self).core.full(),
            M::CHECKED ==> final(self).core.safe_wf(),
            final(self).core.checked == old(self).core.checked,
            final(self).dummy == old(self).dummy,
            final(self).worker == old(self).worker,
            !M::CHECKED ==> r is Ok,
            r is Err ==> final(self).core == old(self).core,
            r matches Ok(last) ==> (c ==> final(self).core.released(
                old(self).core,
                h@.addr,
                edge,
                last,
            )),
            final(self).core.dom() == old(self).core.dom(),
            forall|b: usize|
                b != h@.addr ==> #[trigger] final(self).core.perms@[b] == old(self).core.perms@[b],
            forall|b: usize|
                b != h@.addr ==> #[trigger] final(self).core.state(b) == old(self).core.state(b),
            forall|w: Word| #[trigger] final(self).core.word_ok(w) == old(self).core.word_ok(w),
            final(self).core.model@.building == old(self).core.model@.building,
            final(self).core.active() == old(self).core.active(),
            r matches Ok(last) ==> old(self).core.state(h@.addr) == BlockState::Live
                && final(self).core.state(h@.addr) == if last {
                BlockState::Dying
            } else {
                BlockState::Live
            },
            final(self).core.perm(h@.addr).meta == old(self).core.perm(h@.addr).meta,
            M::CHECKED && old(self).decref_passes(h@.addr) ==> r is Ok,
            M::CHECKED && old(self).decref_twice_passes(h@.addr) && r is Ok ==> r == Ok::<
                bool,
                HeapError,
            >(false) && final(self).decref_passes(h@.addr),
    {
        let counted = self.checked_decref(h)?;
        let ghost s0 = self.core;
        proof {
            s0.lemma_shape(h@.addr);
            if c {
                s0.lemma_at(h@.addr);
            }
            if M::CHECKED {
                reveal(Core::building_wf);
                assert(s0.state(h@.addr) == BlockState::Live);
                s0.lemma_code_state(h@.addr);
            }
        }
        let state = if M::CHECKED {
            self.core.prefix_state_get(h)
        } else {
            0
        };
        let n = self.core.rc_get(h);
        let last = match rc::decref_step(n) {
            DecrefStep::Store(m) => {
                self.core.rc_put(h, m);
                false
            },
            DecrefStep::Free => true,
            DecrefStep::Shared => self.core.rc_down_shared(h),
            DecrefStep::Skip => false,
        };
        proof {
            if c {
                let rc = s0.rc_of(h@.addr);
                let o = Obj { rc, count: s0.model@.refs[h@.addr], stuck: s0.model@.stuck[h@.addr] };
                assert(last == (rc != 0 && decref_next(o).freed));
            }
        }
        // 最後の参照と決めた物体は、検査モードでは前置きも「空ける途中」にする。同じ物体にもう一度届いたときに、
        // 生きていない物体として断るためである (docs/spec/object-model.md の「検査モードの約束」)
        if M::CHECKED && last {
            proof {
                check::lemma_set_code(state, 0);
            }
            self.core.prefix_state_put(h, state & !3u32);
        }
        self.core.count_ghost(
            Ghost(s0),
            Ghost(h@.addr),
            Ghost(false),
            Ghost(edge),
            Ghost(c),
            Ghost(last),
        );
        let ghost s1 = self.core;
        if M::CHECKED && n == 0 && counted {
            self.count_immortal(h, false);
            proof {
                if c {
                    self.core.lemma_recounted_released(s1, s0, h@.addr, edge);
                }
            }
        }
        proof {
            assert(self.core.model == s1.model);
            assert forall|b: usize| b != h@.addr implies #[trigger] self.core.state(b) == s0.state(
                b,
            ) by {
                assert(s1.state(b) == s0.state(b));
            }
            assert(self.core.active() =~= s0.active());
        }
        if counted {
            self.stats.rc_decrements = self.stats.rc_decrements.wrapping_add(1);
        }
        Ok(last)
    }

    /// 最後の参照を失った `root` を空け、それで最後の参照を失った物体を続けて空ける。
    // `decref` の速い道から呼ぶので、展開させずに速い道を小さく保つ
    #[inline(never)]
    fn free_chain(&mut self, root: *mut Header, Ghost(c): Ghost<bool>) -> (r: Result<(), HeapError>)
        requires
            !M::CHECKED ==> c,
            c ==> old(self).core.full(),
            M::CHECKED ==> old(self).core.safe_wf(),
            old(self).core.checked@ == M::CHECKED,
            old(self).core.dom().contains(root@.addr),
            root == old(self).core.block(root@.addr),
            old(self).core.state(root@.addr) == BlockState::Dying,
            c ==> old(self).core.edges_complete(root@.addr),
            c ==> forall|b: usize|
                #![trigger old(self).core.state(b)]
                old(self).core.dom().contains(b) && b != root@.addr ==> old(self).core.state(b)
                    != BlockState::Dying,
            old(self).core.model@.building is None,
        ensures
            M::CHECKED ==> final(self).core.dom() == old(self).core.dom(),
            M::CHECKED ==> final(self).core.pages == old(self).core.pages,
            final(self).core.model@.tokens == old(self).core.model@.tokens,
            final(self).tokens == old(self).tokens,
            !M::CHECKED ==> r is Ok,
            c ==> r is Ok || chain_err(r),
            c ==> final(self).core.full(),
            M::CHECKED ==> final(self).core.safe_wf(),
            final(self).core.checked@ == M::CHECKED,
            c && r is Ok ==> final(self).core.quiet() && final(self).core.evolved(
                old(self).core,
                0,
                0,
            ),
            final(self).core.model@.building is None,
            final(self).dummy == old(self).dummy,
            final(self).worker == old(self).worker,
    {
        let mut work = Vec::new();
        mem::swap(&mut work, &mut self.work);
        work.clear();
        let w = Word::from_ptr(root);
        proof {
            self.core.lemma_shape(root@.addr);
            lemma_aligned_is_ptr(root@.addr);
        }
        work.push(w);
        proof {
            assert(work@[0] == w);
            if c {
                assert forall|b: usize|
                    #![trigger self.core.state(b)]
                    self.core.dom().contains(b) && self.core.state(b) == BlockState::Dying
                        && None::<usize> != Some(b) implies listed(work@, b) by {
                    assert(work@[0].bits_spec() == b);
                }
            }
        }
        let r = self.drain(&mut work, Ghost(c));
        work.clear();
        mem::swap(&mut work, &mut self.work);
        r
    }

    /// 最後の参照を失った物体を空け、それで最後の参照を失った子を作業リストに積む。子は走査の逆の順に手放す。最初の
    /// フィールドを先に、最後のフィールド (cons セルの残りのリスト) を後に空けて、作業リストを短く保つためである。
    fn drain(&mut self, work: &mut Vec<Word>, Ghost(c): Ghost<bool>) -> (r: Result<(), HeapError>)
        requires
            !M::CHECKED ==> c,
            c ==> old(self).core.full(),
            c ==> old(self).core.work_ok(old(work)@),
            M::CHECKED ==> old(self).core.safe_wf(),
            M::CHECKED ==> old(self).core.safe_work(old(work)@, None),
            old(self).core.checked@ == M::CHECKED,
            old(self).core.model@.building is None,
        ensures
            M::CHECKED ==> final(self).core.dom() == old(self).core.dom(),
            M::CHECKED ==> final(self).core.pages == old(self).core.pages,
            final(self).core.model@.tokens == old(self).core.model@.tokens,
            final(self).tokens == old(self).tokens,
            !M::CHECKED ==> r is Ok,
            c ==> r is Ok || chain_err(r),
            c ==> final(self).core.full(),
            M::CHECKED ==> final(self).core.safe_wf(),
            final(self).core.checked@ == M::CHECKED,
            c && r is Ok ==> final(self).core.quiet() && final(self).core.evolved(
                old(self).core,
                0,
                0,
            ),
            final(self).core.tokens_kept(old(self).core),
            final(self).core.model@.building is None,
            final(self).dummy == old(self).dummy,
            final(self).worker == old(self).worker,
    {
        let ghost s0 = self.core;
        proof {
            if c {
                s0.lemma_evolved_self(0);
            }
        }
        // `while let` では取り出す前の作業リストを ghost の値に取れないので、`loop` と `match` で書く
        loop
            invariant
                !M::CHECKED ==> c,
                M::CHECKED ==> self.core.dom() == old(self).core.dom(),
                M::CHECKED ==> self.core.pages == old(self).core.pages,
                self.tokens == old(self).tokens,
                self.core.model@.tokens == old(self).core.model@.tokens,
                c ==> self.core.full(),
                M::CHECKED ==> self.core.safe_wf(),
                self.core.checked@ == M::CHECKED,
                c ==> self.core.work_ok(work@),
                M::CHECKED ==> self.core.safe_work(work@, None),
                c ==> self.core.evolved(s0, 0, 0),
                self.core.tokens_kept(s0),
                s0 == old(self).core,
                self.core.model@.building is None,
                self.dummy == old(self).dummy,
                self.worker == old(self).worker,
            ensures
                work@.len() == 0,
            decreases self.core.active().len(),
        {
            let ghost s1 = self.core;
            let ghost before = work@;
            let w = match work.pop() {
                Some(w) => w,
                None => break,
            };
            proof {
                if c {
                    self.core.lemma_work_pop(before, work@, w);
                }
                if M::CHECKED {
                    self.core.lemma_safe_work_pop(before, work@, w);
                }
            }
            let h = self.header_of(w);
            proof {
                self.core.lemma_shape(h@.addr);
            }
            let (first, n) = self.core.scan_get(h);
            proof {
                if c {
                    self.core.lemma_draining_start(h, w, first as int, n as int, work@);
                }
            }
            let mut i = n;
            while i > 0
                invariant
                    !M::CHECKED ==> c,
                    M::CHECKED ==> self.core.dom() == old(self).core.dom(),
                    M::CHECKED ==> self.core.pages == old(self).core.pages,
                    self.tokens == old(self).tokens,
                    self.core.model@.tokens == old(self).core.model@.tokens,
                    c ==> self.core.full(),
                    M::CHECKED ==> self.core.safe_wf(),
                    self.core.checked@ == M::CHECKED,
                    c ==> self.core.draining(h, first as int, i as int, Set::empty(), work@),
                    M::CHECKED ==> self.core.safe_work(work@, Some(h@.addr)),
                    self.core.dom().contains(h@.addr),
                    h == self.core.block(h@.addr),
                    self.core.state(h@.addr) == BlockState::Dying,
                    first == p_first(self.core.perm(h@.addr)),
                    n == p_count(self.core.perm(h@.addr)),
                    first + n <= self.core.cap(h@.addr),
                    i <= n,
                    c ==> self.core.evolved(s0, 0, 0),
                    self.core.tokens_kept(s0),
                    s0 == old(self).core,
                    self.core.model@.building is None,
                    self.core.active() == s1.active(),
                    self.dummy == old(self).dummy,
                    self.worker == old(self).worker,
                decreases i,
            {
                i -= 1;
                proof {
                    if c {
                        self.core.lemma_draining_slot(
                            h,
                            first as int,
                            i as int,
                            Set::empty(),
                            work@,
                        );
                    }
                    if M::CHECKED {
                        self.core.lemma_safe_at(h@.addr);
                        assert(p_scanned(self.core.perm(h@.addr), (first + i) as int));
                    }
                }
                let child = self.core.slot_get(h, first + i);
                if child.is_ptr() {
                    let c_h = self.header_of(child);
                    let ghost s3 = self.core;
                    let ghost wb = work@;
                    let last = self.release_ref(
                        c_h,
                        Ghost(Some((h@.addr, (first + i) as int))),
                        Ghost(c),
                    )?;
                    if last {
                        work.push(child);
                    }
                    proof {
                        self.core.lemma_tokens_kept_step(s3, s0, c_h@.addr);
                        if c {
                            self.core.lemma_draining_step(
                                s3,
                                s0,
                                h,
                                first as int,
                                i as int,
                                Set::empty(),
                                child,
                                last,
                                wb,
                                work@,
                            );
                        }
                        if M::CHECKED {
                            self.core.lemma_safe_work_step(s3, wb, work@, h@.addr, child, last);
                        }
                    }
                } else {
                    proof {
                        if c {
                            self.core.lemma_draining_skip(
                                h,
                                first as int,
                                i as int,
                                Set::empty(),
                                work@,
                            );
                        }
                    }
                }
            }
            let ghost pre = self.core;
            self.free_box(h, Ghost(c));
            proof {
                self.core.lemma_tokens_kept_step(pre, s0, h@.addr);
                if c {
                    self.core.lemma_drained_one(pre, s0, h, first as int, Set::empty(), work@);
                }
                if M::CHECKED {
                    self.core.lemma_safe_work_freed(pre, work@, h@.addr);
                }
                assert(pre.active().contains(h@.addr));
                vstd::set::lemma_set_remove_len(pre.active(), h@.addr);
            }
        }
        proof {
            if c {
                self.core.lemma_work_empty(work@);
            }
        }
        Ok(())
    }

    /// 箱を空ける。`Foreign` なら破棄処理を呼ぶ。破棄処理が panic しても同じ値を二度破棄しないように、箱を先に
    /// 死んだことにしてから呼ぶ (箱は漏れてよい)。検査モードは箱を使い回さず、前置きを「解放」にして隔離する。
    // 展開すると `release` と `release_reuse` が大きくなり、呼ぶ側に展開されなくなる。そうなると、呼ぶ側が渡す `keep`
    // の定数で枝を畳めない
    #[inline(never)]
    fn free_box(&mut self, h: *mut Header, Ghost(c): Ghost<bool>)
        requires
            !M::CHECKED ==> c,
            c ==> old(self).core.full(),
            c ==> forall|j: int|
                !(#[trigger] old(self).core.model@.edges.dom().contains((h@.addr, j))),
            M::CHECKED ==> old(self).core.safe_wf(),
            old(self).core.checked@ == M::CHECKED,
            old(self).core.dom().contains(h@.addr),
            h == old(self).core.block(h@.addr),
            old(self).core.state(h@.addr) == BlockState::Dying,
        ensures
            M::CHECKED ==> final(self).core.pages == old(self).core.pages,
            final(self).core.model@.tokens == old(self).core.model@.tokens,
            final(self).tokens == old(self).tokens,
            c ==> final(self).core.full(),
            M::CHECKED ==> final(self).core.safe_wf(),
            c ==> final(self).core.boxed_off(old(self).core, h@.addr),
            final(self).core.checked == old(self).core.checked,
            final(self).core.active() == old(self).core.active().remove(h@.addr),
            forall|b: usize|
                b != h@.addr && old(self).core.dom().contains(b)
                    ==> final(self).core.dom().contains(b) && #[trigger] final(self).core.perms@[b]
                    == old(self).core.perms@[b],
            forall|b: usize|
                b != h@.addr ==> #[trigger] final(self).core.state(b) == old(self).core.state(b),
            forall|w: Word|
                old(self).core.word_ok(w) && (w.is_ptr_spec() ==> w.bits_spec() != h@.addr)
                    ==> #[trigger] final(self).core.word_ok(w),
            M::CHECKED ==> final(self).core.dom() == old(self).core.dom(),
            M::CHECKED ==> forall|w: Word| #[trigger]
                final(self).core.word_ok(w) == old(self).core.word_ok(w),
            final(self).core.model@.building == old(self).core.model@.building,
            final(self).dummy == old(self).dummy,
            final(self).worker == old(self).worker,
    {
        let ghost s0 = self.core;
        let meta = self.core.meta_get(h);
        let finalizer = if meta.has_finalizer() {
            let tracked p = self.core.perms.borrow().tracked_borrow(h@.addr);
            proof {
                s0.lemma_shape(h@.addr);
            }
            Some(finalizer_of(h, Tracked(p)))
        } else {
            None
        };
        proof {
            s0.lemma_shape(h@.addr);
            if c {
                s0.lemma_at(h@.addr);
                reveal(Core::refs_wf);
                assert(s0.counted_wf(h@.addr));
            }
        }
        let tracked mut perm = self.core.perms.borrow_mut().tracked_remove(h@.addr);
        if M::CHECKED {
            let tracked mut pre = perm.prefix.tracked_unwrap();
            mark_freed(h, Tracked(&mut pre));
            proof {
                perm.prefix = Some(pre);
            }
            self.quarantined = self.quarantined.wrapping_add(self.core.pages.block_bytes(h));
            self.stats.live = self.stats.live.wrapping_sub(1);
        } else if finalizer.is_some() {
            ptr_mut_write(meta_ptr(h), Tracked(&mut perm.meta), FREE_META);
        }
        if let Some(f) = finalizer {
            run_finalizer(f, Tracked(&perm));
        }
        if !M::CHECKED {
            proof {
                self.core.model = Ghost(
                    Model {
                        state: s0.model@.state.remove(h@.addr),
                        refs: s0.model@.refs.remove(h@.addr),
                        stuck: s0.model@.stuck.remove(h@.addr),
                        owner: s0.model@.owner.remove(h@.addr),
                        written: s0.model@.written.remove(h@.addr),
                        ..s0.model@
                    },
                );
            }
            self.core.pages.free(h, Tracked(perm));
            proof {
                self.core.lemma_block_released(s0, h@.addr);
                assert(self.core.active() =~= s0.active().remove(h@.addr));
            }
        } else {
            proof {
                self.core.perms.borrow_mut().tracked_insert(h@.addr, perm);
                self.core.model = Ghost(
                    Model {
                        state: s0.model@.state.insert(h@.addr, BlockState::Freed),
                        ..s0.model@
                    },
                );
                assert(self.core.perms@.dom() =~= s0.perms@.dom());
                if c {
                    self.core.lemma_block_freed(s0, h@.addr);
                }
                self.core.lemma_safe_freed(s0, h@.addr);
                assert(self.core.active() =~= s0.active().remove(h@.addr));
                assert forall|w: Word| #[trigger] self.core.word_ok(w) == s0.word_ok(w) by {}
            }
        }
    }
}

impl<M: Mode> Heap<M> {
    pub closed spec fn meta_of(&self, w: Word) -> u32 {
        self.core.meta_bits(w.bits_spec())
    }

    /// `w` の i 語目は走査する語である。
    pub closed spec fn scanned_at(&self, w: Word, i: int) -> bool {
        self.core.scanned(w.bits_spec(), i)
    }

    /// `w` の i 語目は生の語である。種類はペイロードが不透明な種類 (`Foreign`、`String`) でなく、i 語目は走査する語の後でブロックの中にある。
    pub closed spec fn raw_at(&self, w: Word, i: int) -> bool {
        let h = w.bits_spec();
        &&& !(self.core.kind(h) == Kind::Foreign || self.core.kind(h) == Kind::String)
        &&& p_first(self.core.perm(h)) + p_count(self.core.perm(h)) <= i < self.core.cap(h)
    }

    /// `w` の物理的な i 語目は書いてある。
    pub closed spec fn written_at(&self, w: Word, i: int) -> bool {
        p_init(self.core.perm(w.bits_spec()), i)
    }

    /// `w` の物理的な i 語目の値。
    pub closed spec fn word_at(&self, w: Word, i: int) -> Word {
        p_word(self.core.perm(w.bits_spec()), i)
    }

    /// `w` の走査する語の i 番目の値。
    pub closed spec fn scanned_word(&self, w: Word, i: int) -> Word {
        let p = self.core.perm(w.bits_spec());
        p_word(p, p_first(p) + i)
    }

    /// `tobj` の `Int` の値。即値ならその値、物体なら `BigInt` の中身である。
    pub closed spec fn int_value(&self, w: Word) -> i64 {
        if w.is_imm_spec() {
            crate::word::imm_value_spec(w.bits_spec())
        } else {
            p_word(self.core.perm(w.bits_spec()), 0).bits_spec() as i64
        }
    }

    /// `w` の物体は、ほかのスレッドが読めない。構築中か、rc が正の生きている物体である。
    pub closed spec fn writable(&self, w: Word) -> bool {
        let h = w.bits_spec();
        &&& self.core.word_ok(w)
        &&& self.core.state(h) == BlockState::Building || (self.core.state(h) == BlockState::Live
            && self.core.rc_of(h) > 0)
    }

    /// `set_field` の後の状態。i 語目に `v` を書き、`v` の外の参照を辺に移し、前の i 語目の辺を外の参照に移す。
    pub closed spec fn field_set(&self, old: Heap<M>, w: Word, i: int, v: Word) -> bool {
        &&& self.dummy == old.dummy
        &&& self.worker == old.worker
        &&& self.core.field_set(old.core, w.bits_spec(), i, v)
    }

    /// `set_raw` の後の状態。i 語目に `v` のビット列を書き、ほかは変えない。
    pub closed spec fn raw_set(&self, old: Heap<M>, w: Word, i: int, v: u64) -> bool {
        &&& self.dummy == old.dummy
        &&& self.worker == old.worker
        &&& self.core.slot_changed(old.core, w.bits_spec(), i)
        &&& p_word(self.core.perm(w.bits_spec()), i).bits_spec() == v as usize
        &&& self.core.model == old.core.model
    }

    /// 不死の物体の参照を1つ作る (docs/spec/object-model.md の「不死の物体の数え方」)。速いモードでは何もしない。
    ///
    /// # Safety
    /// `w` は即値か、このヒープの生きている不死の物体 (rc 0) を指す。
    pub unsafe fn acquire_immortal(&mut self, w: Word) -> (r: Result<(), HeapError>)
        requires
            old(self).wf(),
            old(self).word_in(w),
            !M::CHECKED ==> old(self).settleable() && old(self).readable(w) && (w.is_ptr_spec()
                ==> old(self).rc_value(w) == 0),
        ensures
            final(self).wf(),
            final(self).same_objects(*old(self)),
            old(self).consistent() ==> final(self).consistent(),
            old(self).consistent() && old(self).settleable() && old(self).readable(w) && (
            w.is_ptr_spec() ==> old(self).rc_value(w) == 0) ==> r is Ok,
            r is Err ==> *final(self) == *old(self),
    {
        proof {
            self.core.lemma_recounted_refl();
        }
        if M::CHECKED && w.is_ptr() {
            let h = self.header_of(w);
            proof {
                self.lemma_checks_pass(w);
            }
            self.checked_live(h)?;
            if self.core.rc_get(h) != 0 {
                return Err(HeapError::NotImmortal);
            }
            if self.immortal_counted(h) {
                let ghost s0 = *self;
                self.count_immortal(h, true);
                proof {
                    self.lemma_recounted_heap(s0);
                }
                self.stats.rc_increments = self.stats.rc_increments.wrapping_add(1);
            }
        }
        Ok(())
    }

    /// rc が 1 か。reuse とその場の書き換えに使う。共有、不死、張り付いた数は、この比較でみな偽になる。
    ///
    /// # Safety
    /// `w` は即値か、このヒープの生きている物体を指す。rc が正なら、持ち主は今の worker である。
    pub unsafe fn is_unique(&self, w: Word) -> (r: Result<bool, HeapError>)
        requires
            self.wf(),
            self.word_in(w),
            !M::CHECKED ==> self.settleable() && self.readable(w) && self.owned(w),
        ensures
            r matches Ok(u) ==> u == (w.is_ptr_spec() && self.rc_value(w) == 1),
            self.consistent() && self.settleable() && self.readable(w) && self.owned(w) ==> r is Ok,
    {
        if !w.is_ptr() {
            return Ok(false);
        }
        let h = self.header_of(w);
        proof {
            self.lemma_checks_pass(w);
        }
        self.checked_live(h)?;
        let rc = self.core.rc_get(h);
        self.checked_owner(h, rc)?;
        Ok(rc == 1)
    }

    /// 手放そうとしている参照が最後のものか (rc が 1 か -1)。フィールドを `dup` せずに受け取ってよいかに使う。
    ///
    /// # Safety
    /// `w` は即値か、このヒープの生きている物体を指す。rc が正なら、持ち主は今の worker である。
    pub unsafe fn is_last(&self, w: Word) -> (r: Result<bool, HeapError>)
        requires
            self.wf(),
            self.word_in(w),
            !M::CHECKED ==> self.settleable() && self.readable(w) && self.owned(w),
        ensures
            r matches Ok(l) ==> l == (w.is_ptr_spec() && (self.rc_value(w) == 1 || self.rc_value(w)
                == -1)),
            self.consistent() && self.settleable() && self.readable(w) && self.owned(w) ==> r is Ok,
    {
        if !w.is_ptr() {
            return Ok(false);
        }
        let h = self.header_of(w);
        proof {
            self.lemma_checks_pass(w);
        }
        self.checked_live(h)?;
        let last = self.core.rc_last(h);
        if M::CHECKED {
            let rc = self.core.rc_get(h);
            self.checked_owner(h, rc)?;
        }
        Ok(last)
    }

    /// # Safety
    /// `w` はこのヒープの生きている物体を指す。
    pub unsafe fn meta(&self, w: Word) -> (r: Result<Meta, HeapError>)
        requires
            self.wf(),
            w.is_ptr_spec(),
            self.word_in(w),
            !M::CHECKED ==> self.settleable() && self.readable(w),
        ensures
            r matches Ok(m) ==> m.bits_spec() == self.meta_of(w),
            self.consistent() && self.settleable() && self.readable(w) ==> r is Ok,
    {
        let h = self.header_of(w);
        proof {
            if self.consistent() && self.settleable() && self.readable(w) {
                self.core.lemma_readable(w);
            }
        }
        self.checked_live(h)?;
        Ok(self.core.meta_get(h))
    }

    /// i が走査する語の物理的な添字でなければ `OutOfBounds` を返す。速いモードでは何も確かめない。
    #[inline]
    fn scanned_check(&self, h: *mut Header, i: usize) -> (r: Result<(), HeapError>)
        requires
            self.core.sound(),
            self.core.dom().contains(h@.addr),
            h == self.core.block(h@.addr),
            self.core.state(h@.addr) != BlockState::Freed,
        ensures
            !M::CHECKED ==> r is Ok,
            r is Ok && M::CHECKED ==> self.core.scanned(h@.addr, i as int),
            self.core.scanned(h@.addr, i as int) ==> r is Ok,
    {
        if M::CHECKED {
            let (first, n) = self.core.scan_get(h);
            if i < first || i - first >= n {
                return Err(HeapError::OutOfBounds);
            }
        }
        Ok(())
    }

    /// i が生の語の物理的な添字でなければ `OutOfBounds` を、物体のペイロードが不透明なら `WrongKind` を返す。速いモードでは何も
    /// 確かめない。
    #[inline]
    fn raw_check(&self, h: *mut Header, i: usize) -> (r: Result<(), HeapError>)
        requires
            self.core.sound(),
            self.core.dom().contains(h@.addr),
            h == self.core.block(h@.addr),
            self.core.state(h@.addr) != BlockState::Freed,
        ensures
            !M::CHECKED ==> r is Ok,
            r is Ok && M::CHECKED ==> {
                let p = self.core.perm(h@.addr);
                &&& !(self.core.kind(h@.addr) == Kind::Foreign || self.core.kind(h@.addr)
                    == Kind::String)
                &&& p_first(p) + p_count(p) <= i < self.core.cap(h@.addr)
            },
            ({
                let p = self.core.perm(h@.addr);
                &&& !(self.core.kind(h@.addr) == Kind::Foreign || self.core.kind(h@.addr)
                    == Kind::String)
                &&& p_first(p) + p_count(p) <= i < self.core.cap(h@.addr)
            }) ==> r is Ok,
    {
        if M::CHECKED {
            if self.core.meta_get(h).kind().opaque() {
                return Err(HeapError::WrongKind);
            }
            let (first, n) = self.core.scan_get(h);
            let cap = self.core.cap_get(h);
            if i < first + n || i >= cap {
                return Err(HeapError::OutOfBounds);
            }
        }
        Ok(())
    }

    /// ペイロードの物理的な i 語目を読む。
    ///
    /// # Safety
    /// `w` はこのヒープの生きている物体を指し、i 語目はペイロードの中の走査する語である。
    pub unsafe fn field(&self, w: Word, i: usize) -> (r: Result<Word, HeapError>)
        requires
            self.wf(),
            self.safe_settleable(),
            w.is_ptr_spec(),
            self.word_in(w),
            !M::CHECKED ==> self.settleable() && self.readable(w) && self.scanned_at(w, i as int),
        ensures
            r matches Ok(v) ==> v == self.word_at(w, i as int) && self.word_in(v),
            self.consistent() && self.settleable() && self.readable(w) && self.scanned_at(
                w,
                i as int,
            ) ==> r is Ok,
            r matches Ok(v) ==> (self.consistent() && self.settleable() && self.readable(w)
                ==> self.readable(v)),
    {
        let h = self.header_of(w);
        proof {
            if self.consistent() && self.settleable() && self.readable(w) {
                self.core.lemma_readable(w);
            }
        }
        self.checked_live(h)?;
        self.scanned_check(h, i)?;
        proof {
            if self.consistent() && self.settleable() && self.readable(w) && self.scanned_at(
                w,
                i as int,
            ) {
                self.core.lemma_live_slot(h@.addr, i as int);
            }
            if M::CHECKED {
                self.core.lemma_safe_live_slot(h@.addr, i as int);
            }
        }
        Ok(self.core.slot_get(h, i))
    }

    /// ペイロードの物理的な i 語目を書く。参照の数は変えない (所有の受け渡しは呼ぶ側が決める)。
    ///
    /// # Safety
    /// `w` はこのヒープの生きている物体を指し、i 語目はペイロードの中の走査する語で、ほかのスレッドはその物体を読めない。
    /// `v` は即値か、このヒープの生きている物体を指し、呼ぶ側がその参照を1つ持つ。その参照は `w` の i 語目に移る。
    /// rc が正なら、持ち主は今の worker である。
    pub unsafe fn set_field(&mut self, w: Word, i: usize, v: Word) -> (r: Result<(), HeapError>)
        requires
            old(self).wf(),
            w.is_ptr_spec(),
            old(self).word_in(w),
            old(self).unshared(w),
            old(self).word_in(v),
            !M::CHECKED ==> old(self).writable(w) && old(self).holds(v) && old(self).scanned_at(
                w,
                i as int,
            ) && old(self).owned(w),
        ensures
            final(self).wf(),
            r is Err ==> *final(self) == *old(self),
            r is Ok ==> final(self).field_set(*old(self), w, i as int, v),
            old(self).consistent() && old(self).writable(w) && old(self).holds(v) && old(
                self,
            ).scanned_at(w, i as int) && old(self).owned(w) ==> r is Ok && final(self).consistent(),
    {
        let h = self.header_of(w);
        let ghost c = self.consistent() && self.writable(w) && self.holds(v) && self.scanned_at(
            w,
            i as int,
        ) && self.owned(w);
        proof {
            self.lemma_checks_pass(w);
        }
        self.checked_live(h)?;
        self.scanned_check(h, i)?;
        if M::CHECKED {
            let rc = self.core.rc_get(h);
            self.checked_owner(h, rc)?;
        }
        let ghost s0 = self.core;
        proof {
            s0.lemma_shape(h@.addr);
            if c {
                s0.lemma_at(h@.addr);
                s0.lemma_view_indeg();
            }
        }
        self.core.slot_put(h, i, v);
        proof {
            self.core.model = Ghost(
                Model {
                    edges: field_edges(s0.model@.edges, h@.addr, i as int, v),
                    written: if s0.state(h@.addr) == BlockState::Building {
                        s0.model@.written.insert(
                            h@.addr,
                            s0.model@.written[h@.addr].insert(i as int),
                        )
                    } else {
                        s0.model@.written
                    },
                    kept: s0.model@.kept.remove((h@.addr, i as int)),
                    ..s0.model@
                },
            );
            if c {
                self.core.lemma_field_set(s0, h@.addr, i as int, v);
                assert(self.core.perms@[self.dummy.bits_spec()]
                    == s0.perms@[self.dummy.bits_spec()]);
            }
            if M::CHECKED {
                assert(self.core.word_ok(v) == s0.word_ok(v));
                self.core.lemma_safe_slot_set(s0, h@.addr, i as int);
            }
        }
        Ok(())
    }

    /// # Safety
    /// `w` はこのヒープの生きている物体を指し、i 語目はペイロードの中の生の語で、すでに書いてある。`alloc_data` は生の
    /// 語を書かず、新しいページのメモリは初期化されていないので、書く前に読むと未初期化の読み込みになる。この条件は
    /// 検査モードも確かめない。
    pub unsafe fn raw(&self, w: Word, i: usize) -> (r: Result<u64, HeapError>)
        requires
            self.wf(),
            w.is_ptr_spec(),
            self.word_in(w),
            self.written_at(w, i as int),
            !M::CHECKED ==> self.settleable() && self.readable(w) && self.raw_at(w, i as int),
        ensures
            r matches Ok(x) ==> x == self.word_at(w, i as int).bits_spec() as u64,
            self.consistent() && self.settleable() && self.readable(w) && self.raw_at(w, i as int)
                ==> r is Ok,
    {
        let h = self.header_of(w);
        proof {
            if self.consistent() && self.settleable() && self.readable(w) {
                self.core.lemma_readable(w);
            }
        }
        self.checked_live(h)?;
        self.raw_check(h, i)?;
        let x = self.core.slot_get(h, i);
        Ok(x.bits() as u64)
    }

    /// # Safety
    /// `w` はこのヒープの生きている物体を指し、i 語目はペイロードの中の生の語で、ほかのスレッドはその物体を読めない。
    /// rc が正なら、持ち主は今の worker である。
    pub unsafe fn set_raw(&mut self, w: Word, i: usize, v: u64) -> (r: Result<(), HeapError>)
        requires
            old(self).wf(),
            w.is_ptr_spec(),
            old(self).word_in(w),
            old(self).unshared(w),
            !M::CHECKED ==> old(self).writable(w) && old(self).raw_at(w, i as int) && old(
                self,
            ).owned(w),
        ensures
            final(self).wf(),
            r is Err ==> *final(self) == *old(self),
            r is Ok ==> final(self).raw_set(*old(self), w, i as int, v),
            old(self).consistent() && old(self).writable(w) && old(self).raw_at(w, i as int) && old(
                self,
            ).owned(w) ==> r is Ok && final(self).consistent(),
    {
        let h = self.header_of(w);
        let ghost c = self.consistent() && self.writable(w) && self.raw_at(w, i as int)
            && self.owned(w);
        proof {
            self.lemma_checks_pass(w);
        }
        self.checked_live(h)?;
        self.raw_check(h, i)?;
        if M::CHECKED {
            let rc = self.core.rc_get(h);
            self.checked_owner(h, rc)?;
        }
        let ghost s0 = self.core;
        proof {
            s0.lemma_shape(h@.addr);
        }
        self.core.slot_put(h, i, Word::from_bits(v as usize));
        proof {
            if c {
                self.core.lemma_raw_set(s0, h@.addr, i as int);
                assert(self.core.perms@[self.dummy.bits_spec()]
                    == s0.perms@[self.dummy.bits_spec()]);
            }
            if M::CHECKED {
                self.core.lemma_safe_slot_set(s0, h@.addr, i as int);
            }
        }
        Ok(())
    }

    /// 走査する語の数。形を知らない処理が使う。
    ///
    /// # Safety
    /// `w` はこのヒープの生きている物体を指す。
    pub unsafe fn scanned_len(&self, w: Word) -> (r: Result<usize, HeapError>)
        requires
            self.wf(),
            self.safe_settleable(),
            w.is_ptr_spec(),
            self.word_in(w),
            !M::CHECKED ==> self.settleable() && self.readable(w),
        ensures
            r matches Ok(n) ==> n == self.count_of(w),
            self.consistent() && self.settleable() && self.readable(w) ==> r is Ok,
    {
        let h = self.header_of(w);
        proof {
            if self.consistent() && self.settleable() && self.readable(w) {
                self.core.lemma_readable(w);
            }
        }
        self.checked_live(h)?;
        proof {
            if M::CHECKED {
                self.core.lemma_safe_live_block(h@.addr);
            }
        }
        let (_, n) = self.core.scan_get(h);
        Ok(n)
    }

    /// 走査する語の i 番目を読む。`scan` が 255 のときの数の語と、種類の走査の始まりを考える。
    ///
    /// # Safety
    /// `w` はこのヒープの生きている物体を指し、i は `scanned_len(w)` より小さい。
    pub unsafe fn scanned(&self, w: Word, i: usize) -> (r: Result<Word, HeapError>)
        requires
            self.wf(),
            self.safe_settleable(),
            w.is_ptr_spec(),
            self.word_in(w),
            !M::CHECKED ==> self.settleable() && self.readable(w) && i < self.count_of(w),
        ensures
            r matches Ok(v) ==> v == self.scanned_word(w, i as int) && self.word_in(v),
            self.consistent() && self.settleable() && self.readable(w) && i < self.count_of(w)
                ==> r is Ok,
            r matches Ok(v) ==> (self.consistent() && self.settleable() && self.readable(w)
                ==> self.readable(v)),
    {
        let h = self.header_of(w);
        proof {
            if self.consistent() && self.settleable() && self.readable(w) {
                self.core.lemma_readable(w);
            }
        }
        self.checked_live(h)?;
        let (first, n) = self.core.scan_get(h);
        if M::CHECKED && i >= n {
            return Err(HeapError::OutOfBounds);
        }
        proof {
            if self.consistent() && self.settleable() && self.readable(w) && i < self.count_of(w) {
                self.core.lemma_live_slot(h@.addr, (first + i) as int);
            }
            if M::CHECKED {
                self.core.lemma_safe_live_slot(h@.addr, (first + i) as int);
            }
        }
        Ok(self.core.slot_get(h, first + i))
    }

    /// 使っているメモリのバイト数 (docs/spec/object-model.md の「確保と解放」)。検査モードでは隔離したブロックを引く。
    pub fn bytes_in_use(&self) -> usize {
        // 隔離したブロックは確保したページの一部なので、引き算は桁を下回らない。この関係はまだ証明していないので、
        // 下回ったら 0 にする
        self.core.pages.bytes().saturating_sub(self.quarantined)
    }

    /// `T` の値を持つ `Foreign` の物体を作る。値の破棄処理は、物体を空けるときか `Heap` を落とすときに1回だけ呼ぶ。
    pub fn alloc_foreign<T: Send + 'static>(&mut self, value: T, lin: bool) -> (w: Word)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            !M::CHECKED ==> old(self).settleable(),
        ensures
            final(self).made(*old(self), w),
            final(self).kind_of(w) == Kind::Foreign,
    {
        let n = foreign::value_words::<T>();
        self.settle();
        let ghost s1 = *self;
        let (h, Tracked(perm)) = self.alloc_header(1 + n, 1, Meta::new(0, 0, Kind::Foreign, lin));
        let tracked mut perm = perm;
        proof {
            self.core.pages.lemma_block_bounds(h@.addr);
        }
        let cap = self.core.pages.cap_of(h);
        foreign::write(h, cap, value, Tracked(&mut perm));
        let ghost mid = self.core;
        proof {
            check::lemma_alive_code();
            let c = self.core;
            let a = h@.addr;
            assert(perm.wf(c.block(a), c.cap(a), c.checked@));
            assert(perm.prefix matches Some(q) ==> q.first.is_init() && q.state.is_init());
            assert(perm.prefix is Some ==> p_code(perm) == state_code(BlockState::Live));
            assert(perm.prefix is Some && perm.rc.value() > 0 ==> p_owner(perm) == self.worker);
            assert(bits_wf(p_meta(perm)));
            assert(p_kind(perm) != Kind::Free);
            assert(p_first(perm) + p_count(perm) <= c.cap(a));
            assert(c.perm_wf(perm, a, BlockState::Live, Set::empty(), self.worker));
        }
        self.core.publish(Ghost(h), Tracked(perm), Ghost(BlockState::Live), Ghost(self.worker));
        let w = Word::from_ptr(h);
        proof {
            if old(self).consistent() && old(self).settleable() {
                self.lemma_dummy_kept(s1);
                self.core.lemma_frame_chain(mid, s1.core, old(self).core);
            } else {
                self.lemma_published_safe(s1);
            }
            crate::word::lemma_aligned_is_ptr(h@.addr);
        }
        w
    }

    /// `Foreign` の物体の Rust の値の位置。物体が `Foreign` でないか、値が `T` でなければ `WrongKind` を返す。この照らし
    /// 合わせは、速いモードでも行う (docs/spec/object-model.md の「核の操作」)。
    ///
    /// # Safety
    /// `w` はこのヒープの生きている物体を指す。
    pub unsafe fn foreign<T: 'static>(&self, w: Word) -> (r: Result<*mut T, HeapError>)
        requires
            self.wf(),
            w.is_ptr_spec(),
            self.word_in(w),
            !M::CHECKED ==> self.settleable() && self.readable(w),
        ensures
            r is Err ==> M::CHECKED || r == Err::<*mut T, HeapError>(HeapError::WrongKind),
    {
        let h = self.header_of(w);
        proof {
            if self.consistent() && self.settleable() && self.readable(w) {
                self.core.lemma_readable(w);
            }
        }
        self.checked_live(h)?;
        if self.core.meta_get(h).kind() != Kind::Foreign {
            return Err(HeapError::WrongKind);
        }
        let tracked p = self.core.perms.borrow().tracked_borrow(h@.addr);
        match foreign::value::<T>(h, Tracked(p)) {
            Some(v) => Ok(v),
            None => Err(HeapError::WrongKind),
        }
    }

    /// `tobj` の `Int` を読む。即値なら右シフトし、物体なら `BigInt` の中身を読む。
    ///
    /// # Safety
    /// `w` は即値か、このヒープの生きている `BigInt` の物体 (ダミーを含む) を指す。
    pub unsafe fn unbox_int(&self, w: Word) -> (r: Result<i64, HeapError>)
        requires
            self.wf(),
            self.safe_settleable(),
            w.is_imm_spec() || w.is_ptr_spec(),
            self.word_in(w),
            !M::CHECKED ==> self.settleable() && self.readable(w) && (w.is_ptr_spec()
                ==> self.kind_of(w) == Kind::BigInt),
        ensures
            r matches Ok(n) ==> n == self.int_value(w),
            self.consistent() && self.settleable() && self.readable(w) && (w.is_ptr_spec()
                ==> self.kind_of(w) == Kind::BigInt) ==> r is Ok,
    {
        if w.is_imm() {
            return Ok(w.imm_value());
        }
        let h = self.header_of(w);
        proof {
            if self.consistent() && self.settleable() && self.readable(w) {
                self.core.lemma_readable(w);
            }
        }
        self.checked_live(h)?;
        if M::CHECKED && !matches!(self.core.meta_get(h).kind(), Kind::BigInt) {
            return Err(HeapError::WrongKind);
        }
        proof {
            if self.consistent() && self.settleable() && self.readable(w) {
                self.core.lemma_live_block(h@.addr);
            }
            if M::CHECKED {
                self.core.lemma_safe_live_block(h@.addr);
            }
        }
        let x = self.core.slot_get(h, 0);
        Ok(x.bits() as i64)
    }
}

} // verus!
