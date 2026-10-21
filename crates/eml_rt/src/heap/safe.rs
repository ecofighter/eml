//! 検査モードの安全の不変条件 `safe_wf` (docs/spec/object-model.md の「検査モードの約束」)。権限の持ち方とブロックの
//! 状態だけを述べ、モデルの数と辺と持ち主には触れない。検査モードは空けたブロックを使い回さず権限も手放さないので、
//! モデルの前提を破った呼び出しの後もこれが成り立ち、メモリの安全はこれだけから示せる。

use vstd::prelude::*;

use super::Core;
#[cfg(verus_only)]
use super::check::UNCOUNTED;
#[cfg(verus_only)]
use super::inv::{
    body_wf, p_code, p_count, p_extended, p_filled, p_first, p_gen, p_init, p_kind, p_meta,
    p_owner, p_scanned, p_word, prefix_kept, prefix_recounted, state_code,
};
#[cfg(verus_only)]
use super::model::{BlockState, Model, Pos};
#[cfg(verus_only)]
use crate::meta::{BlockPerm, Header, Kind, bits_wf};
#[cfg(verus_only)]
use crate::word::Word;

verus! {

impl Core {
    pub(crate) open spec fn safe_wf(&self) -> bool {
        &&& self.dom() == self.pages.in_use()
        &&& self.safe_rest()
    }

    /// 確保器から取ったブロック `h` を、まだヒープに入れていない。
    pub(crate) open spec fn safe_pending(&self, h: usize) -> bool {
        &&& self.pages.in_use() == self.dom().insert(h)
        &&& !self.dom().contains(h)
        &&& self.safe_rest()
    }

    pub(crate) open spec fn safe_rest(&self) -> bool {
        &&& self.pages.wf()
        &&& self.pages.prefixed() == self.checked()
        &&& self.env_wf()
        &&& self.safe_blocks_wf()
        &&& self.building_wf()
        &&& self.safe_tokens_wf()
    }

    /// 検査モードの token の表 (docs/spec/object-model.md の「検査モードの token」)。項目があれば、その箱は token の
    /// 状態で、世代の番号は項目のものに等しい。添字を使い回さずに積み、取り出すと空にするので、2つの項目が同じ箱を
    /// 指すことはない。
    #[verifier::opaque]
    pub(crate) open spec fn safe_tokens_wf(&self) -> bool {
        let t = self.model@.tokens;
        &&& forall|i: int|
            0 <= i < t.len() && (#[trigger] t[i]) is Some ==> self.token_entry(t[i]->0)
        &&& forall|i: int, j: int|
            0 <= i < t.len() && 0 <= j < t.len() && i != j && (#[trigger] t[i]) is Some && (
            #[trigger] t[j]) is Some ==> (t[i]->0).0@.addr != (t[j]->0).0@.addr
    }

    pub(crate) open spec fn token_entry(&self, e: (*mut Header, u32)) -> bool {
        &&& self.dom().contains(e.0@.addr)
        &&& e.0 == self.block(e.0@.addr)
        &&& self.state(e.0@.addr) == BlockState::Token
        &&& p_gen(self.perm(e.0@.addr)) == e.1
    }

    /// 箱 `h` を指す token の表の項目がある。
    pub(crate) open spec fn registered(&self, h: usize) -> bool {
        exists|i: int|
            0 <= i < self.model@.tokens.len() && (#[trigger] self.model@.tokens[i]) is Some && (
            self.model@.tokens[i]->0).0@.addr == h
    }

    /// token の表の項目が指す箱は、token の箱である。
    pub(crate) proof fn lemma_registered(&self, h: usize)
        requires
            self.safe_tokens_wf(),
            self.registered(h),
        ensures
            self.dom().contains(h),
            self.state(h) == BlockState::Token,
    {
        reveal(Core::safe_tokens_wf);
        let i = choose|i: int|
            0 <= i < self.model@.tokens.len() && (#[trigger] self.model@.tokens[i]) is Some && (
            self.model@.tokens[i]->0).0@.addr == h;
        assert(self.token_entry(self.model@.tokens[i]->0));
    }

    #[verifier::opaque]
    pub(crate) open spec fn safe_blocks_wf(&self) -> bool {
        &&& self.model@.state.dom() == self.dom()
        &&& self.model@.written.dom() == self.dom()
        &&& forall|h: usize| #[trigger] self.dom().contains(h) ==> self.safe_block_wf(h)
        &&& forall|e: Pos| #[trigger] self.model@.kept.contains(e) ==> self.safe_kept_wf(e)
    }

    pub(crate) open spec fn safe_block_wf(&self, h: usize) -> bool {
        &&& self.pages.carved_block(h)
        &&& self.safe_perm_wf(self.perm(h), h, self.state(h), self.model@.written[h])
    }

    /// 構築中の物体と token の箱の「残した」語は書いてあり、即値か、このヒープのブロックを指す。
    pub(crate) open spec fn safe_kept_wf(&self, e: Pos) -> bool {
        &&& self.kept_wf(e)
        &&& self.word_ok(p_word(self.perm(e.0), e.1))
    }

    /// i 語目は書いてあり、値は即値か、このヒープのブロックを指す。
    pub(crate) open spec fn safe_slot(&self, p: BlockPerm, i: int) -> bool {
        &&& p_init(p, i)
        &&& self.word_ok(p_word(p, i))
    }

    /// `perm_wf` のうち、モデルの数と辺と持ち主に触れない部分。空ける途中の物体と token の箱の走査する語も、解放の連鎖と
    /// 箱の使い回しが読むので、即値か、このヒープのブロックを指す。
    pub(crate) open spec fn safe_perm_wf(
        &self,
        p: BlockPerm,
        h: usize,
        st: BlockState,
        written: Set<int>,
    ) -> bool {
        &&& p.wf(self.block(h), self.cap(h), self.checked())
        &&& p.prefix matches Some(q) ==> q.first.is_init() && q.state.is_init()
        &&& p.prefix is Some ==> p_code(p) == state_code(st)
        &&& p.rc.is_init()
        &&& p.meta.is_init()
        &&& bits_wf(p_meta(p))
        &&& p_kind(p) != Kind::Free
        &&& st != BlockState::Freed ==> {
            &&& p_extended(p) ==> self.cap(h) >= 1 && p_init(p, 0)
            &&& p_first(p) + p_count(p) <= self.cap(h)
        }
        &&& st == BlockState::Live || st == BlockState::Dying ==> (p_kind(p).boxes_value()
            ==> self.cap(h) >= 1 && p_init(p, 0))
        &&& st == BlockState::Live || st == BlockState::Dying ==> (p_kind(p).has_body() ==> body_wf(
            p,
            self.cap(h),
        ))
        &&& st != BlockState::Freed && p_kind(p) == Kind::Data ==> p_filled(p, self.cap(h))
        &&& st == BlockState::Building ==> !p_kind(p).has_body()
        &&& st == BlockState::Live || st == BlockState::Dying || st == BlockState::Token ==> forall|
            i: int,
        | #[trigger]
            p_scanned(p, i) ==> self.safe_slot(p, i)
        &&& st == BlockState::Building ==> forall|i: int| #[trigger]
            written.contains(i) ==> p_scanned(p, i) && self.safe_slot(p, i)
        &&& st == BlockState::Token ==> p_kind(p) == Kind::Data
    }

    /// `old` から、ブロック `h` の権限と状態と書いた語と「残した」語だけが変わった。ほかのブロックとページは同じである。
    pub(crate) open spec fn safe_step(&self, old: Core, h: usize) -> bool {
        &&& self.pages == old.pages
        &&& self.env == old.env
        &&& self.perms@.dom() == old.perms@.dom()
        &&& old.dom().contains(h)
        &&& forall|b: usize| b != h ==> #[trigger] self.perms@[b] == old.perms@[b]
        &&& self.model@.state.dom() == old.model@.state.dom()
        &&& forall|b: usize| b != h ==> #[trigger] self.model@.state[b] == old.model@.state[b]
        &&& self.model@.written.dom() == old.model@.written.dom()
        &&& forall|b: usize| b != h ==> #[trigger] self.model@.written[b] == old.model@.written[b]
        &&& forall|e: Pos|
            e.0 != h ==> (#[trigger] self.model@.kept.contains(e) == old.model@.kept.contains(e))
        &&& self.model@.tokens == old.model@.tokens
    }

    /// ブロック `h` のほかを変えない変化は、`h` が安全の条件を満たせば `safe_wf` を保つ。
    pub(crate) proof fn lemma_safe_step(&self, old: Core, h: usize)
        requires
            old.safe_wf(),
            self.safe_step(old, h),
            self.safe_block_wf(h),
            forall|i: int| #[trigger]
                self.model@.kept.contains((h, i)) ==> self.safe_kept_wf((h, i)),
            self.building_wf(),
            old.registered(h) ==> self.state(h) == BlockState::Token && p_gen(self.perm(h))
                == p_gen(old.perm(h)),
        ensures
            self.safe_wf(),
            forall|w: Word| #[trigger] self.word_ok(w) == old.word_ok(w),
    {
        reveal(Core::safe_tokens_wf);
        let t = self.model@.tokens;
        assert forall|i: int|
            0 <= i < t.len() && (#[trigger] t[i]) is Some implies self.token_entry(t[i]->0) by {
            assert(old.token_entry(t[i]->0));
            if (t[i]->0).0@.addr == h {
                assert(old.registered(h));
            }
        }
        reveal(Core::safe_blocks_wf);
        assert forall|w: Word| #[trigger] self.word_ok(w) == old.word_ok(w) by {}
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.safe_block_wf(b) by {
            if b != h {
                assert(old.safe_block_wf(b));
                let p = self.perm(b);
                assert forall|i: int| #[trigger]
                    p_scanned(p, i) && old.safe_slot(p, i) implies self.safe_slot(p, i) by {
                    assert(self.word_ok(p_word(p, i)) == old.word_ok(p_word(p, i)));
                }
                assert forall|i: int| #[trigger]
                    self.model@.written[b].contains(i) && old.safe_slot(
                        p,
                        i,
                    ) implies self.safe_slot(p, i) by {
                    assert(self.word_ok(p_word(p, i)) == old.word_ok(p_word(p, i)));
                }
            }
        }
        assert forall|e: Pos| #[trigger] self.model@.kept.contains(e) implies self.safe_kept_wf(
            e,
        ) by {
            if e.0 != h {
                assert(old.model@.kept.contains(e));
                assert(old.safe_kept_wf(e));
                assert(self.word_ok(p_word(self.perm(e.0), e.1)) == old.word_ok(
                    p_word(old.perm(e.0), e.1),
                ));
            } else {
                assert(self.safe_kept_wf((h, e.1)));
            }
        }
    }

    /// 確保器が変わっても、ヒープのブロックの形が同じなら、安全の条件の残りは変わらない。
    pub(crate) proof fn lemma_safe_pages_frame(&self, s0: Core)
        requires
            s0.safe_rest(),
            self.perms == s0.perms,
            self.model == s0.model,
            self.env == s0.env,
            self.pages.wf(),
            self.pages.prefixed() == s0.pages.prefixed(),
            self.env_wf(),
            forall|b: usize| #[trigger]
                s0.dom().contains(b) ==> self.block(b) == s0.block(b) && self.cap(b) == s0.cap(b)
                    && self.pages.carved_block(b),
        ensures
            self.safe_rest(),
            forall|w: Word| #[trigger] self.word_ok(w) == s0.word_ok(w),
    {
        reveal(Core::safe_blocks_wf);
        reveal(Core::building_wf);
        reveal(Core::safe_tokens_wf);
        assert forall|i: int|
            0 <= i < self.model@.tokens.len() && (
            #[trigger] self.model@.tokens[i]) is Some implies self.token_entry(
            self.model@.tokens[i]->0,
        ) by {
            assert(s0.token_entry(self.model@.tokens[i]->0));
        }
        assert forall|w: Word| #[trigger] self.word_ok(w) == s0.word_ok(w) by {
            if w.is_ptr_spec() && s0.dom().contains(w.bits_spec()) {
                assert(self.block(w.bits_spec()) == s0.block(w.bits_spec()));
            }
        }
        assert forall|h: usize| #[trigger] self.dom().contains(h) implies self.safe_block_wf(h) by {
            assert(s0.safe_block_wf(h));
            let p = self.perm(h);
            assert forall|i: int| #[trigger]
                p_scanned(p, i) && s0.safe_slot(p, i) implies self.safe_slot(p, i) by {
                assert(self.word_ok(p_word(p, i)) == s0.word_ok(p_word(p, i)));
            }
            assert forall|i: int| #[trigger]
                self.model@.written[h].contains(i) && s0.safe_slot(p, i) implies self.safe_slot(
                p,
                i,
            ) by {
                assert(self.word_ok(p_word(p, i)) == s0.word_ok(p_word(p, i)));
            }
        }
        assert forall|e: Pos| #[trigger] self.model@.kept.contains(e) implies self.safe_kept_wf(
            e,
        ) by {
            assert(s0.safe_kept_wf(e));
            assert(self.word_ok(p_word(self.perm(e.0), e.1)) == s0.word_ok(
                p_word(s0.perm(e.0), e.1),
            ));
        }
    }

    /// `full` の下では、token の箱でないブロックは安全の条件を満たす。空ける途中の物体は、走査する語のポインタがみな辺で
    /// あれば満たす。
    pub(crate) proof fn lemma_safe_block_full(&self, h: usize)
        requires
            self.full(),
            self.dom().contains(h),
            self.state(h) != BlockState::Token,
            self.state(h) == BlockState::Dying ==> self.edges_complete(h),
        ensures
            self.safe_block_wf(h),
    {
        self.lemma_at(h);
        reveal(Core::edges_wf);
        let p = self.perm(h);
        if self.state(h) == BlockState::Dying {
            assert forall|i: int| #[trigger] p_scanned(p, i) implies self.safe_slot(p, i) by {
                assert(self.scanned(h, i));
                if p_word(p, i).is_ptr_spec() {
                    assert(self.edge_wf((h, i)));
                }
            }
        }
        if self.state(h) == BlockState::Live {
            assert forall|i: int| #[trigger] p_scanned(p, i) implies self.safe_slot(p, i) by {
                assert(self.slot_word(p, h, i));
            }
        }
        if self.state(h) == BlockState::Building {
            assert forall|i: int| #[trigger] self.model@.written[h].contains(i) implies p_scanned(
                p,
                i,
            ) && self.safe_slot(p, i) by {
                assert(self.slot_word(p, h, i));
            }
        }
    }

    /// 確保器から取ったブロック `h` をヒープに入れた後も、`h` が安全の条件を満たせば `safe_wf` を保つ。
    pub(crate) proof fn lemma_safe_publish(&self, s0: Core, h: usize)
        requires
            s0.safe_pending(h),
            self.pages == s0.pages,
            self.env == s0.env,
            self.perms@ == s0.perms@.insert(h, self.perm(h)),
            self.model@.state == s0.model@.state.insert(h, self.state(h)),
            self.model@.written == s0.model@.written.insert(h, self.model@.written[h]),
            self.model@.kept == s0.model@.kept,
            self.model@.tokens == s0.model@.tokens,
            self.safe_block_wf(h),
            self.building_wf(),
        ensures
            self.safe_wf(),
            forall|w: Word| s0.word_ok(w) ==> #[trigger] self.word_ok(w),
    {
        reveal(Core::safe_blocks_wf);
        reveal(Core::safe_tokens_wf);
        assert forall|i: int|
            0 <= i < self.model@.tokens.len() && (
            #[trigger] self.model@.tokens[i]) is Some implies self.token_entry(
            self.model@.tokens[i]->0,
        ) by {
            assert(s0.token_entry(self.model@.tokens[i]->0));
            assert((self.model@.tokens[i]->0).0@.addr != h);
        }
        assert(self.dom() =~= self.pages.in_use());
        assert(self.model@.state.dom() =~= self.dom());
        assert(self.model@.written.dom() =~= self.dom());
        assert forall|w: Word| s0.word_ok(w) implies #[trigger] self.word_ok(w) by {}
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.safe_block_wf(b) by {
            if b != h {
                assert(s0.safe_block_wf(b));
                let p = self.perm(b);
                assert(p == s0.perm(b));
                assert forall|i: int| #[trigger]
                    p_scanned(p, i) && s0.safe_slot(p, i) implies self.safe_slot(p, i) by {}
                assert forall|i: int| #[trigger]
                    self.model@.written[b].contains(i) && s0.safe_slot(p, i) implies self.safe_slot(
                    p,
                    i,
                ) by {}
            }
        }
        assert forall|e: Pos| #[trigger] self.model@.kept.contains(e) implies self.safe_kept_wf(
            e,
        ) by {
            assert(s0.safe_kept_wf(e));
            assert(e.0 != h);
            assert(self.perm(e.0) == s0.perm(e.0));
        }
    }

    /// 空のヒープは、安全の条件を満たす。
    pub(crate) proof fn lemma_safe_empty(&self)
        requires
            self.pages.wf(),
            self.pages.prefixed() == self.checked(),
            self.pages.in_use() == Set::<usize>::empty(),
            self.perms@ == Map::<usize, BlockPerm>::empty(),
            self.model@.state == Map::<usize, BlockState>::empty(),
            self.model@.written == Map::<usize, Set<int>>::empty(),
            self.model@.kept == Set::<Pos>::empty(),
            self.model@.building is None,
            self.model@.tokens == Seq::<Option<(*mut Header, u32)>>::empty(),
            self.env@.region is None,
        ensures
            self.safe_wf(),
    {
        reveal(Core::safe_blocks_wf);
        reveal(Core::safe_tokens_wf);
        reveal(Core::building_wf);
        assert(self.dom() =~= self.pages.in_use());
    }

    /// 書き終えた構築中の物体を生きている物体にしても、`safe_wf` は保たれる。
    pub(crate) proof fn lemma_safe_settle(&self, s0: Core, b: usize)
        requires
            s0.safe_wf(),
            s0.safe_settleable(),
            s0.model@.building == Some(b),
            *self == s0.settled_view(),
        ensures
            self.safe_wf(),
            forall|w: Word| #[trigger] self.word_ok(w) == s0.word_ok(w),
    {
        reveal(Core::safe_blocks_wf);
        reveal(Core::building_wf);
        assert(s0.dom().contains(b));
        assert(s0.safe_block_wf(b));
        let p = s0.perm(b);
        assert forall|i: int| #[trigger] p_scanned(p, i) implies self.safe_slot(p, i) by {
            assert(s0.scanned(b, i));
            if !s0.model@.written[b].contains(i) {
                assert(s0.model@.kept.contains((b, i)));
                assert(s0.safe_kept_wf((b, i)));
            }
        }
        if p_kind(p).boxes_value() {
            assert(p.slots.contains_key(0));
        }
        assert(self.model@.state =~= s0.model@.state.insert(b, self.state(b)));
        assert(self.model@.written =~= s0.model@.written.insert(b, self.model@.written[b]));
        assert(self.building_wf());
        if s0.registered(b) {
            s0.lemma_registered(b);
        }
        self.lemma_safe_step(s0, b);
    }

    /// `old` から、rc と前置きの `first` の欄の値だけが変わった (`share` と不死の物体の数)。
    pub(crate) open spec fn values_changed(&self, old: Core) -> bool {
        &&& self.pages == old.pages
        &&& self.env == old.env
        &&& self.model@.state == old.model@.state
        &&& self.model@.written == old.model@.written
        &&& self.model@.kept == old.model@.kept
        &&& self.model@.building == old.model@.building
        &&& self.model@.tokens == old.model@.tokens
        &&& self.perms@.dom() == old.perms@.dom()
        &&& forall|b: usize| #[trigger]
            old.dom().contains(b) ==> {
                &&& self.perm(b).rc.ptr() == old.perm(b).rc.ptr()
                &&& self.perm(b).rc.is_init()
                &&& self.perm(b).meta == old.perm(b).meta
                &&& self.perm(b).slots == old.perm(b).slots
                &&& self.perm(b).text == old.perm(b).text
                &&& prefix_recounted(self.perm(b), old.perm(b), true)
            }
    }

    pub(crate) proof fn lemma_recounted_values(&self, old: Core)
        requires
            old.safe_wf(),
            self.recounted(old),
        ensures
            self.values_changed(old),
    {
        reveal(Core::safe_blocks_wf);
        assert forall|b: usize| #[trigger] old.dom().contains(b) implies prefix_recounted(
            self.perm(b),
            old.perm(b),
            true,
        ) by {
            assert(old.safe_block_wf(b));
        }
    }

    /// 検査モードで空ける途中のブロックを「解放」にして隔離しても、`safe_wf` は保たれる。
    pub(crate) proof fn lemma_safe_freed(&self, s0: Core, h: usize)
        requires
            s0.safe_wf(),
            s0.dom().contains(h),
            s0.state(h) == BlockState::Dying,
            self.pages == s0.pages,
            self.env == s0.env,
            self.perms@.dom() == s0.perms@.dom(),
            forall|b: usize| b != h ==> #[trigger] self.perms@[b] == s0.perms@[b],
            self.perm(h).rc == s0.perm(h).rc,
            self.perm(h).meta == s0.perm(h).meta,
            self.perm(h).slots == s0.perm(h).slots,
            self.perm(h).text == s0.perm(h).text,
            self.perm(h).prefix is Some,
            s0.perm(h).prefix is Some,
            self.perm(h).prefix->0.first == s0.perm(h).prefix->0.first,
            self.perm(h).prefix->0.state.ptr() == s0.perm(h).prefix->0.state.ptr(),
            self.perm(h).prefix->0.state.is_init(),
            p_code(self.perm(h)) == state_code(BlockState::Freed),
            self.model@ == (Model {
                state: s0.model@.state.insert(h, BlockState::Freed),
                ..s0.model@
            }),
        ensures
            self.safe_wf(),
            forall|w: Word| #[trigger] self.word_ok(w) == s0.word_ok(w),
    {
        reveal(Core::safe_blocks_wf);
        reveal(Core::building_wf);
        assert(s0.safe_block_wf(h));
        assert forall|i: int| !#[trigger] self.model@.kept.contains((h, i)) by {
            if s0.model@.kept.contains((h, i)) {
                assert(s0.safe_kept_wf((h, i)));
            }
        }
        if s0.registered(h) {
            s0.lemma_registered(h);
        }
        assert(self.model@.state.dom() =~= s0.model@.state.dom());
        self.lemma_safe_step(s0, h);
    }

    /// ブロック `h` の状態を替えた後 (空ける途中、token の箱、構築中の物体にする)。語は替えず、前の状態の走査する語は
    /// 安全の条件を満たすので、替えた後に読む走査する語と「残した」語が前の走査する語に入っていればよい。
    pub(crate) proof fn lemma_safe_restate(&self, s0: Core, h: usize)
        requires
            s0.safe_wf(),
            self.safe_step(s0, h),
            s0.state(h) == BlockState::Live || s0.state(h) == BlockState::Dying || s0.state(h)
                == BlockState::Token,
            self.state(h) == BlockState::Building ==> self.model@.building == Some(h)
                && s0.model@.building is None,
            self.state(h) != BlockState::Building ==> self.model@.building == s0.model@.building,
            self.state(h) != BlockState::Freed,
            self.perm(h).slots == s0.perm(h).slots,
            self.perm(h).text == s0.perm(h).text,
            self.perm(h).rc.ptr() == s0.perm(h).rc.ptr(),
            self.perm(h).rc.is_init(),
            self.perm(h).meta.ptr() == s0.perm(h).meta.ptr(),
            self.perm(h).meta.is_init(),
            bits_wf(p_meta(self.perm(h))),
            p_kind(self.perm(h)) != Kind::Free,
            (self.perm(h).prefix is Some) == (s0.perm(h).prefix is Some),
            self.perm(h).prefix matches Some(q) ==> {
                &&& q.first.ptr() == s0.perm(h).prefix->0.first.ptr()
                &&& q.state.ptr() == s0.perm(h).prefix->0.state.ptr()
                &&& q.first.is_init()
                &&& q.state.is_init()
                &&& p_code(self.perm(h)) == state_code(self.state(h))
            },
            p_extended(self.perm(h)) ==> self.cap(h) >= 1 && p_init(self.perm(h), 0),
            p_first(self.perm(h)) + p_count(self.perm(h)) <= self.cap(h),
            self.state(h) != BlockState::Building ==> self.perm(h).meta == s0.perm(h).meta,
            self.state(h) == BlockState::Building ==> self.model@.written[h] == Set::<int>::empty(),
            self.state(h) == BlockState::Token ==> p_kind(self.perm(h)) == Kind::Data,
            self.state(h) == BlockState::Building ==> !p_kind(self.perm(h)).has_body(),
            p_kind(self.perm(h)) == Kind::Data ==> p_kind(s0.perm(h)) == Kind::Data,
            forall|i: int| #[trigger]
                self.model@.kept.contains((h, i)) ==> p_scanned(self.perm(h), i) && p_scanned(
                    s0.perm(h),
                    i,
                ) && (self.state(h) == BlockState::Building || self.state(h) == BlockState::Token),
            !s0.registered(h),
        ensures
            self.safe_wf(),
            forall|w: Word| #[trigger] self.word_ok(w) == s0.word_ok(w),
    {
        reveal(Core::safe_blocks_wf);
        reveal(Core::building_wf);
        assert(s0.safe_block_wf(h));
        assert forall|b: usize| #[trigger]
            self.dom().contains(b) && self.state(b)
                == BlockState::Building implies self.model@.building == Some(b) by {
            if b != h {
                assert(s0.state(b) == self.state(b));
            }
        }
        assert(self.building_wf());
        let p = self.perm(h);
        let q = s0.perm(h);
        assert forall|i: int| #[trigger] p_scanned(q, i) implies self.safe_slot(p, i) by {
            assert(s0.safe_slot(q, i));
        }
        assert forall|i: int| #[trigger]
            self.model@.kept.contains((h, i)) implies self.safe_kept_wf((h, i)) by {
            assert(s0.safe_slot(q, i));
        }
        if self.state(h) != BlockState::Building {
            assert forall|i: int| #[trigger] p_scanned(p, i) implies self.safe_slot(p, i) by {
                assert(p_scanned(q, i));
            }
        }
        assert(p.wf(self.block(h), self.cap(h), self.checked()));
        if p_kind(p) == Kind::Data {
            assert(p_filled(q, s0.cap(h)));
        }
        assert(self.safe_perm_wf(p, h, self.state(h), self.model@.written[h]));
        assert(self.safe_block_wf(h));
        self.lemma_safe_step(s0, h);
    }

    /// 生きている物体か構築中の物体の i 語目に書いた後。走査する語なら、書いた値は即値か、このヒープのブロックを指し、
    /// 構築中の物体ではその語を書いたことにする。走査する語でなければ (生の語)、モデルは変えない。
    pub(crate) proof fn lemma_safe_slot_set(&self, s0: Core, h: usize, i: int)
        requires
            s0.safe_wf(),
            s0.dom().contains(h),
            s0.state(h) == BlockState::Live || s0.state(h) == BlockState::Building,
            self.slot_changed(s0, h, i),
            s0.scanned(h, i) ==> {
                &&& self.word_ok(p_word(self.perm(h), i))
                &&& self.model@ == Model {
                    written: if s0.state(h) == BlockState::Building {
                        s0.model@.written.insert(h, s0.model@.written[h].insert(i))
                    } else {
                        s0.model@.written
                    },
                    kept: s0.model@.kept.remove((h, i)),
                    edges: self.model@.edges,
                    ..s0.model@
                }
            },
            !s0.scanned(h, i) ==> p_first(s0.perm(h)) + p_count(s0.perm(h)) <= i && self.model
                == s0.model && !s0.kind(h).has_body(),
        ensures
            self.safe_wf(),
            forall|w: Word| #[trigger] self.word_ok(w) == s0.word_ok(w),
    {
        reveal(Core::safe_blocks_wf);
        reveal(Core::building_wf);
        assert(s0.safe_block_wf(h));
        let p = self.perm(h);
        let q = s0.perm(h);
        assert(p_first(p) == p_first(q));
        if p_extended(q) {
            assert(i != 0);
            assert(p.slots[0] == q.slots[0]);
        }
        assert(p_count(p) == p_count(q));
        assert forall|j: int| #[trigger]
            p_scanned(q, j) && j != i && s0.safe_slot(q, j) implies self.safe_slot(p, j) by {
            assert(p.slots[j] == q.slots[j]);
        }
        assert forall|j: int| #[trigger]
            p_scanned(p, j) && s0.state(h) == BlockState::Live implies self.safe_slot(p, j) by {
            assert(p_scanned(q, j));
            if j != i {
                assert(s0.safe_slot(q, j));
            }
        }
        if s0.state(h) == BlockState::Building {
            assert forall|j: int| #[trigger] self.model@.written[h].contains(j) implies p_scanned(
                p,
                j,
            ) && self.safe_slot(p, j) by {
                if j != i {
                    assert(s0.model@.written[h].contains(j));
                    assert(s0.safe_slot(q, j));
                }
            }
        }
        if p_kind(q).boxes_value() && s0.state(h) == BlockState::Live {
            assert(p_init(q, 0));
            assert(p.slots.contains_key(0));
            if i != 0 {
                assert(p.slots[0] == q.slots[0]);
            }
        }
        assert(self.safe_block_wf(h));
        assert forall|j: int| #[trigger]
            self.model@.kept.contains((h, j)) implies self.safe_kept_wf((h, j)) by {
            assert(s0.model@.kept.contains((h, j)));
            assert(s0.safe_kept_wf((h, j)));
            if j == i {
                assert(!s0.scanned(h, i) ==> false);
            }
            assert(p.slots[j] == q.slots[j]);
        }
        assert(self.model@.state.dom() == s0.model@.state.dom());
        assert(self.model@.written.dom() == s0.model@.written.dom());
        assert(forall|b: usize|
            b != h ==> #[trigger] self.model@.written[b] == s0.model@.written[b]);
        assert(forall|e: Pos|
            e.0 != h ==> (#[trigger] self.model@.kept.contains(e) == s0.model@.kept.contains(e)));
        if s0.registered(h) {
            s0.lemma_registered(h);
        }
        self.lemma_safe_step(s0, h);
    }

    pub(crate) proof fn lemma_values_refl(&self)
        requires
            self.sound(),
        ensures
            self.values_changed(*self),
    {
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies prefix_recounted(
            self.perm(b),
            self.perm(b),
            true,
        ) && self.perm(b).rc.is_init() by {
            self.lemma_shape(b);
        }
    }

    pub(crate) proof fn lemma_values_trans(&self, mid: Core, old: Core)
        requires
            mid.values_changed(old),
            self.values_changed(mid),
        ensures
            self.values_changed(old),
    {
        assert forall|b: usize| #[trigger] old.dom().contains(b) implies {
            &&& self.perm(b).rc.ptr() == old.perm(b).rc.ptr()
            &&& self.perm(b).rc.is_init()
            &&& self.perm(b).meta == old.perm(b).meta
            &&& self.perm(b).slots == old.perm(b).slots
            &&& self.perm(b).text == old.perm(b).text
            &&& prefix_recounted(self.perm(b), old.perm(b), true)
        } by {
            assert(mid.dom().contains(b));
        }
    }

    /// `h` の rc を書き、モデルの数を動かした後 (`dup`)。値のほかは変わらない。
    pub(crate) proof fn lemma_values_rc(&self, mid: Core, old: Core, h: usize)
        requires
            old.sound(),
            old.has(h),
            old.state(h) == BlockState::Live,
            mid.rc_changed(old, h),
            mid.perm(h).prefix == old.perm(h).prefix,
            self.perms == mid.perms,
            self.pages == old.pages,
            self.env == old.env,
            self.model@.state == if old.dom().contains(h) {
                old.model@.state.insert(h, BlockState::Live)
            } else {
                old.model@.state
            },
            self.model@.written == old.model@.written,
            self.model@.kept == old.model@.kept,
            self.model@.building == old.model@.building,
            self.model@.tokens == old.model@.tokens,
        ensures
            self.values_changed(old),
    {
        if old.full() {
            reveal(Core::blocks_wf);
        } else {
            reveal(Core::safe_blocks_wf);
        }
        assert(self.model@.state =~= old.model@.state);
        assert forall|b: usize| #[trigger] old.dom().contains(b) implies {
            &&& self.perm(b).rc.ptr() == old.perm(b).rc.ptr()
            &&& self.perm(b).rc.is_init()
            &&& self.perm(b).meta == old.perm(b).meta
            &&& self.perm(b).slots == old.perm(b).slots
            &&& self.perm(b).text == old.perm(b).text
            &&& prefix_recounted(self.perm(b), old.perm(b), true)
        } by {
            old.lemma_shape(b);
            if b != h {
                assert(self.perms@[b] == old.perms@[b]);
            }
        }
    }

    /// rc と前置きの `first` の欄の値は、安全の条件に入らない。
    pub(crate) proof fn lemma_safe_values(&self, old: Core)
        requires
            old.safe_wf(),
            self.values_changed(old),
        ensures
            self.safe_wf(),
            forall|w: Word| #[trigger] self.word_ok(w) == old.word_ok(w),
    {
        reveal(Core::safe_blocks_wf);
        reveal(Core::building_wf);
        reveal(Core::safe_tokens_wf);
        assert forall|i: int|
            0 <= i < self.model@.tokens.len() && (
            #[trigger] self.model@.tokens[i]) is Some implies self.token_entry(
            self.model@.tokens[i]->0,
        ) by {
            assert(old.token_entry(self.model@.tokens[i]->0));
        }
        assert forall|w: Word| #[trigger] self.word_ok(w) == old.word_ok(w) by {}
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.safe_block_wf(b) by {
            assert(old.safe_block_wf(b));
            let p = self.perm(b);
            let q = old.perm(b);
            assert forall|i: int| #[trigger] p_scanned(p, i) implies p_scanned(q, i) by {}
            assert forall|i: int| #[trigger]
                p_scanned(p, i) && old.safe_slot(q, i) implies self.safe_slot(p, i) by {}
            assert forall|i: int| #[trigger]
                self.model@.written[b].contains(i) && old.safe_slot(q, i) implies self.safe_slot(
                p,
                i,
            ) by {}
        }
        assert forall|e: Pos| #[trigger] self.model@.kept.contains(e) implies self.safe_kept_wf(
            e,
        ) by {
            assert(old.safe_kept_wf(e));
        }
    }

    /// token の表の写しだけを替えても、`full` と、表のほかの安全の条件は変わらない。
    pub(crate) proof fn lemma_tokens_frame(&self, old: Core)
        requires
            self.pages == old.pages,
            self.perms == old.perms,
            self.env == old.env,
            self.model@ == (Model { tokens: self.model@.tokens, ..old.model@ }),
        ensures
            old.full() ==> self.full(),
            old.safe_wf() && self.safe_tokens_wf() ==> self.safe_wf(),
            forall|w: Word| #[trigger] self.word_ok(w) == old.word_ok(w),
            forall|b: usize| #[trigger] self.state(b) == old.state(b),
    {
        reveal(Core::blocks_wf);
        reveal(Core::edges_wf);
        reveal(Core::refs_wf);
        reveal(Core::building_wf);
        reveal(Core::safe_blocks_wf);
        assert forall|w: Word| #[trigger] self.word_ok(w) == old.word_ok(w) by {}
        if old.full() {
            assert forall|h: usize| #[trigger] self.dom().contains(h) implies self.block_wf(h) by {
                assert(old.block_wf(h));
            }
            assert forall|e: Pos| #[trigger] self.model@.kept.contains(e) implies self.kept_wf(
                e,
            ) by {
                assert(old.kept_wf(e));
            }
            assert forall|e: Pos| #[trigger]
                self.model@.edges.dom().contains(e) implies self.edge_wf(e) by {
                assert(old.edge_wf(e));
            }
            assert forall|h: usize| #[trigger] self.dom().contains(h) implies self.counted_wf(
                h,
            ) by {
                assert(old.counted_wf(h));
            }
        }
        if old.safe_wf() {
            assert forall|h: usize| #[trigger] self.dom().contains(h) implies self.safe_block_wf(
                h,
            ) by {
                assert(old.safe_block_wf(h));
            }
            assert forall|e: Pos| #[trigger] self.model@.kept.contains(e) implies self.safe_kept_wf(
                e,
            ) by {
                assert(old.safe_kept_wf(e));
            }
        }
    }

    /// token の表の i 番目の項目を空けると、その箱を指す項目はなくなる。
    pub(crate) proof fn lemma_token_taken(&self, old: Core, i: int)
        requires
            old.safe_wf(),
            0 <= i < old.model@.tokens.len(),
            old.model@.tokens[i] is Some,
            self.pages == old.pages,
            self.perms == old.perms,
            self.env == old.env,
            self.model@ == (Model { tokens: old.model@.tokens.update(i, None), ..old.model@ }),
        ensures
            self.safe_tokens_wf(),
            !self.registered((old.model@.tokens[i]->0).0@.addr),
    {
        reveal(Core::safe_tokens_wf);
        let t = self.model@.tokens;
        let a = (old.model@.tokens[i]->0).0@.addr;
        assert forall|j: int|
            0 <= j < t.len() && (#[trigger] t[j]) is Some implies self.token_entry(t[j]->0) by {
            assert(old.model@.tokens[j] == t[j]);
            assert(old.token_entry(t[j]->0));
        }
        assert forall|j: int, k: int|
            0 <= j < t.len() && 0 <= k < t.len() && j != k && (#[trigger] t[j]) is Some && (
            #[trigger] t[k]) is Some implies (t[j]->0).0@.addr != (t[k]->0).0@.addr by {
            assert(old.model@.tokens[j] == t[j]);
            assert(old.model@.tokens[k] == t[k]);
        }
        if self.registered(a) {
            let j = choose|j: int|
                0 <= j < t.len() && (#[trigger] t[j]) is Some && (t[j]->0).0@.addr == a;
            assert(old.model@.tokens[j] == t[j]);
        }
    }

    /// まだ表にない token の箱を、表の最後に積む。
    pub(crate) proof fn lemma_token_pushed(&self, old: Core, h: *mut Header, g: u32)
        requires
            old.safe_wf(),
            old.token_entry((h, g)),
            !old.registered(h@.addr),
            self.pages == old.pages,
            self.perms == old.perms,
            self.env == old.env,
            self.model@ == (Model { tokens: old.model@.tokens.push(Some((h, g))), ..old.model@ }),
        ensures
            self.safe_tokens_wf(),
    {
        reveal(Core::safe_tokens_wf);
        let t = self.model@.tokens;
        let n = old.model@.tokens.len();
        assert forall|j: int|
            0 <= j < t.len() && (#[trigger] t[j]) is Some implies self.token_entry(t[j]->0) by {
            if j < n {
                assert(old.model@.tokens[j] == t[j]);
                assert(old.token_entry(t[j]->0));
            }
        }
        assert forall|j: int, k: int|
            0 <= j < t.len() && 0 <= k < t.len() && j != k && (#[trigger] t[j]) is Some && (
            #[trigger] t[k]) is Some implies (t[j]->0).0@.addr != (t[k]->0).0@.addr by {
            if j < n && k < n {
                assert(old.model@.tokens[j] == t[j]);
                assert(old.model@.tokens[k] == t[k]);
            } else if j < n {
                assert(old.model@.tokens[j] == t[j]);
                assert(!old.registered(h@.addr));
            } else if k < n {
                assert(old.model@.tokens[k] == t[k]);
                assert(!old.registered(h@.addr));
            }
        }
    }

    /// rc を書き、最後の参照なら空ける途中にした後 (`dying`)。モデルの状態のほかは、安全の条件に入らない。
    pub(crate) proof fn lemma_safe_counted(&self, s0: Core, h: usize, dying: bool)
        requires
            s0.safe_wf(),
            s0.dom().contains(h),
            s0.state(h) == BlockState::Live,
            self.pages == s0.pages,
            self.env == s0.env,
            self.perms@.dom() == s0.perms@.dom(),
            forall|b: usize| b != h ==> #[trigger] self.perms@[b] == s0.perms@[b],
            self.perm(h).meta == s0.perm(h).meta,
            self.perm(h).slots == s0.perm(h).slots,
            self.perm(h).text == s0.perm(h).text,
            prefix_kept(self.perm(h), s0.perm(h)),
            self.perm(h).rc.ptr() == s0.perm(h).rc.ptr(),
            self.perm(h).rc.is_init(),
            self.perm(h).prefix is Some ==> p_code(self.perm(h)) == if dying {
                0u32
            } else {
                1u32
            },
            self.model@.state == s0.model@.state.insert(
                h,
                if dying {
                    BlockState::Dying
                } else {
                    BlockState::Live
                },
            ),
            self.model@.written == s0.model@.written,
            self.model@.kept == s0.model@.kept,
            self.model@.building == s0.model@.building,
            self.model@.tokens == s0.model@.tokens,
        ensures
            self.safe_wf(),
            forall|w: Word| #[trigger] self.word_ok(w) == s0.word_ok(w),
    {
        reveal(Core::safe_blocks_wf);
        reveal(Core::building_wf);
        s0.lemma_safe_at(h);
        if s0.registered(h) {
            s0.lemma_registered(h);
        }
        assert forall|i: int| !#[trigger] self.model@.kept.contains((h, i)) by {
            if s0.model@.kept.contains((h, i)) {
                assert(s0.safe_kept_wf((h, i)));
            }
        }
        assert(self.model@.state.dom() =~= s0.model@.state.dom());
        assert(self.building_wf());
        self.lemma_safe_restate(s0, h);
    }

    /// 構築中の物体はないか、書き終えている。走査する語はどれも「書いた」か「残した」で、`BigInt` と `Float` なら値の
    /// 語が書いてある。`settleable` のうち、安全に要る部分である。
    pub(crate) open spec fn safe_settleable(&self) -> bool {
        match self.model@.building {
            None => true,
            Some(b) => {
                &&& forall|i: int| #[trigger]
                    self.scanned(b, i) ==> self.model@.written[b].contains(i)
                        || self.model@.kept.contains((b, i))
                &&& self.kind(b).boxes_value() ==> p_init(self.perm(b), 0)
            },
        }
    }

    /// 生きている物体と書き終えた構築中の物体の形。
    pub(crate) proof fn lemma_safe_live_block(&self, h: usize)
        requires
            self.safe_wf(),
            self.safe_settleable(),
            self.has(h),
            self.state(h) == BlockState::Live || self.state(h) == BlockState::Building,
        ensures
            p_extended(self.perm(h)) ==> p_init(self.perm(h), 0),
            p_kind(self.perm(h)).boxes_value() ==> self.cap(h) >= 1 && p_init(self.perm(h), 0),
            p_kind(self.perm(h)).has_body() ==> body_wf(self.perm(h), self.cap(h)),
            p_first(self.perm(h)) + p_count(self.perm(h)) <= self.cap(h),
    {
        if !self.dom().contains(h) {
            self.lemma_region_at(h);
            return;
        }
        reveal(Core::building_wf);
        self.lemma_safe_at(h);
        if p_kind(self.perm(h)).boxes_value() {
            assert(self.perm(h).slots.contains_key(0));
        }
    }

    /// 生きている物体と書き終えた構築中の物体の走査する語は書いてあり、即値か、このヒープのブロックを指す。
    pub(crate) proof fn lemma_safe_live_slot(&self, h: usize, i: int)
        requires
            self.safe_wf(),
            self.safe_settleable(),
            self.has(h),
            self.state(h) != BlockState::Freed,
            self.scanned(h, i),
        ensures
            self.safe_slot(self.perm(h), i),
    {
        if !self.dom().contains(h) {
            self.lemma_region_at(h);
            return;
        }
        reveal(Core::building_wf);
        reveal(Core::safe_blocks_wf);
        self.lemma_safe_at(h);
        if self.state(h) == BlockState::Building && !self.model@.written[h].contains(i) {
            assert(self.model@.kept.contains((h, i)));
            assert(self.safe_kept_wf((h, i)));
        }
    }

    /// 確保器から取ったブロック `h` に `perm_wf` を満たす権限を入れた後、`h` は安全の条件を満たす。ヒープの語は増える
    /// だけなので、`perm_wf` の語の条件は入れた後も成り立つ。
    pub(crate) proof fn lemma_perm_safe(
        &self,
        s0: Core,
        perm: BlockPerm,
        h: usize,
        st: BlockState,
        owner: u32,
    )
        requires
            s0.perm_wf(perm, h, st, Set::empty(), owner),
            st == BlockState::Live || st == BlockState::Building,
            s0.pages.carved_block(h),
            !s0.region_has(h),
            self.pages == s0.pages,
            self.env == s0.env,
            self.perm(h) == perm,
            self.state(h) == st,
            self.model@.written[h] == Set::<int>::empty(),
            forall|w: Word| s0.word_ok(w) ==> #[trigger] self.word_ok(w),
        ensures
            self.safe_block_wf(h),
    {
        if st == BlockState::Live {
            assert forall|i: int| #[trigger] p_scanned(perm, i) implies self.safe_slot(perm, i) by {
                assert(s0.slot_word(perm, h, i));
            }
        }
    }

    /// 安全の条件の下では、モデルの状態と書いた語の表は、ヒープのブロックをみな持つ。
    pub(crate) proof fn lemma_safe_doms(&self)
        requires
            self.safe_wf(),
        ensures
            self.model@.state.dom() == self.dom(),
            self.model@.written.dom() == self.dom(),
    {
        reveal(Core::safe_blocks_wf);
    }

    /// どちらかの不変条件が成り立つ。検査モードでモデルが合わなくなっても、`safe_wf` は成り立つ。
    pub(crate) open spec fn sound(&self) -> bool {
        self.full() || self.safe_wf()
    }

    /// `perm_wf` と `safe_perm_wf` に共通する、ブロック `h` の形。読み書きの手順はこれだけに頼る。
    pub(crate) open spec fn block_shape(&self, h: usize) -> bool {
        let p = self.perm(h);
        let st = self.state(h);
        &&& self.dom().contains(h) ==> self.pages.carved_block(h)
        &&& p.wf(self.block(h), self.cap(h), self.checked())
        &&& p.prefix matches Some(q) ==> q.first.is_init() && q.state.is_init()
        &&& p.prefix is Some ==> p_code(p) == state_code(st)
        &&& p.rc.is_init()
        &&& p.meta.is_init()
        &&& bits_wf(p_meta(p))
        &&& p_kind(p) != Kind::Free
        &&& st != BlockState::Freed ==> {
            &&& p_extended(p) ==> self.cap(h) >= 1 && p_init(p, 0)
            &&& p_first(p) + p_count(p) <= self.cap(h)
        }
        &&& st == BlockState::Live || st == BlockState::Dying ==> (p_kind(p).boxes_value()
            ==> self.cap(h) >= 1 && p_init(p, 0))
        &&& st == BlockState::Live || st == BlockState::Dying ==> (p_kind(p).has_body() ==> body_wf(
            p,
            self.cap(h),
        ))
        &&& st == BlockState::Token ==> p_kind(p) == Kind::Data
    }

    /// どちらかの不変条件の下での、ブロック `h` の位置と大きさと形。
    pub(crate) proof fn lemma_shape(&self, h: usize)
        requires
            self.sound(),
            self.has(h),
        ensures
            self.block_shape(h),
            self.dom().contains(h) ==> self.pages.carved_block(h),
            self.block(h)@.addr == h,
            h % 8 == 0,
            h >= 8,
            h + 8 * (1 + self.cap(h)) <= usize::MAX,
    {
        if !self.dom().contains(h) {
            self.lemma_region_at(h);
        } else if self.full() {
            self.lemma_at(h);
        } else {
            self.lemma_safe_at(h);
        }
    }

    /// 付けた塊のブロックは、このヒープのブロックでない。
    pub(crate) proof fn lemma_apart(&self, h: usize)
        requires
            self.sound(),
            self.region_has(h),
        ensures
            !self.dom().contains(h),
            self.in_region(h),
    {
    }

    /// 付けた塊を、その子孫の塊に替えた後 (`Heap::attach`)。ヒープのブロックと前の塊のブロックは、同じ権限と形のまま
    /// 読めるので、不変条件は保たれる。新しく見えるブロックは、ヒープのページと別のページにある。
    pub(crate) proof fn lemma_attached(&self, s0: Core)
        requires
            s0.full() || s0.safe_wf(),
            self.pages == s0.pages,
            self.perms == s0.perms,
            self.model == s0.model,
            self.checked() == s0.checked(),
            self.env@.region is Some,
            self.region_wf(),
            s0.env@.region matches Some(o) ==> self.region().reaches(o@),
            forall|a: usize| #[trigger] self.pages.has_page(a) ==> !self.region().has_page(a),
        ensures
            s0.full() ==> self.full(),
            s0.safe_wf() ==> self.safe_wf(),
            forall|w: Word| #[trigger] s0.word_ok(w) ==> self.word_ok(w),
            forall|b: usize| #[trigger]
                s0.has(b) ==> {
                    &&& self.has(b)
                    &&& self.perm(b) == s0.perm(b)
                    &&& self.state(b) == s0.state(b)
                    &&& self.block(b) == s0.block(b)
                    &&& self.cap(b) == s0.cap(b)
                },
    {
        let r = self.region();
        let prefixed = self.checked();
        if let Some(o) = s0.env@.region {
            r.lemma_reaches(o@, prefixed);
        }
        assert forall|h: usize|
            #![trigger self.region_has(h)]
            #![trigger self.pages.in_use().contains(h)]
            self.region_has(h) implies !self.pages.in_use().contains(h) by {
            if self.pages.in_use().contains(h) {
                self.pages.lemma_in_use_carved(h);
                self.pages.lemma_carved_page(h);
                r.lemma_block(h, prefixed);
            }
        }
        assert(self.env_wf());
        assert forall|b: usize| #[trigger] s0.has(b) implies {
            &&& self.has(b)
            &&& self.perm(b) == s0.perm(b)
            &&& self.state(b) == s0.state(b)
            &&& self.block(b) == s0.block(b)
            &&& self.cap(b) == s0.cap(b)
        } by {
            if !s0.dom().contains(b) {
                assert(s0.region_has(b));
            }
        }
        assert forall|w: Word| #[trigger] s0.word_ok(w) implies self.word_ok(w) by {
            if w.is_ptr_spec() {
                assert(s0.has(w.bits_spec()));
            }
        }
        if s0.full() {
            reveal(Core::blocks_wf);
            reveal(Core::edges_wf);
            reveal(Core::refs_wf);
            reveal(Core::building_wf);
            assert forall|h: usize| #[trigger] self.dom().contains(h) implies self.block_wf(h) by {
                assert(s0.block_wf(h));
                let p = self.perm(h);
                assert forall|i: int| #[trigger]
                    p_scanned(p, i) && s0.slot_word(p, h, i) implies self.slot_word(p, h, i) by {
                    assert(s0.word_ok(p_word(p, i)));
                }
                assert forall|i: int| #[trigger]
                    self.model@.written[h].contains(i) && s0.slot_word(
                        p,
                        h,
                        i,
                    ) implies self.slot_word(p, h, i) by {
                    assert(s0.word_ok(p_word(p, i)));
                }
            }
            assert forall|e: Pos| #[trigger] self.model@.kept.contains(e) implies self.kept_wf(
                e,
            ) by {
                assert(s0.kept_wf(e));
            }
            assert forall|e: Pos| #[trigger]
                self.model@.edges.dom().contains(e) implies self.edge_wf(e) by {
                assert(s0.edge_wf(e));
                assert(s0.has(self.model@.edges[e]));
            }
            assert forall|h: usize| #[trigger] self.dom().contains(h) implies self.counted_wf(
                h,
            ) by {
                assert(s0.counted_wf(h));
            }
        }
        if s0.safe_wf() {
            reveal(Core::safe_blocks_wf);
            reveal(Core::building_wf);
            reveal(Core::safe_tokens_wf);
            assert forall|h: usize| #[trigger] self.dom().contains(h) implies self.safe_block_wf(
                h,
            ) by {
                assert(s0.safe_block_wf(h));
                let p = self.perm(h);
                assert forall|i: int| #[trigger]
                    p_scanned(p, i) && s0.safe_slot(p, i) implies self.safe_slot(p, i) by {
                    assert(s0.word_ok(p_word(p, i)));
                }
                assert forall|i: int| #[trigger]
                    self.model@.written[h].contains(i) && s0.safe_slot(p, i) implies self.safe_slot(
                    p,
                    i,
                ) by {
                    assert(s0.word_ok(p_word(p, i)));
                }
            }
            assert forall|e: Pos| #[trigger] self.model@.kept.contains(e) implies self.safe_kept_wf(
                e,
            ) by {
                assert(s0.safe_kept_wf(e));
                assert(s0.word_ok(p_word(s0.perm(e.0), e.1)));
            }
            assert forall|i: int|
                0 <= i < self.model@.tokens.len() && (
                #[trigger] self.model@.tokens[i]) is Some implies self.token_entry(
                self.model@.tokens[i]->0,
            ) by {
                assert(s0.token_entry(self.model@.tokens[i]->0));
            }
        }
    }

    /// 付けた塊の物体は rc 0 で、検査モードの前置きは生存の状態である。
    pub(crate) proof fn lemma_region_rc(&self, h: usize)
        requires
            self.region_wf(),
            self.in_region(h),
        ensures
            self.rc_of(h) == 0,
            self.perm(h).prefix is Some ==> p_code(self.perm(h)) == 1,
    {
        self.region().lemma_block(h, self.checked());
    }

    /// 付けた塊のブロック `h` の形。生存の rc 0 の物体で、`Foreign` でなく、検査モードの前置きは数えない印を持つ。走査する
    /// 語は書いてあり、即値か、付けた塊のブロックを指す。
    pub(crate) proof fn lemma_region_at(&self, h: usize)
        requires
            self.sound(),
            self.in_region(h),
        ensures
            self.block_shape(h),
            self.block(h)@.addr == h,
            h % 8 == 0,
            h >= 8,
            h + 8 * (1 + self.cap(h)) <= usize::MAX,
            self.state(h) == BlockState::Live,
            self.rc_of(h) == 0,
            p_kind(self.perm(h)) != Kind::Foreign,
            self.checked() ==> self.perm(h).prefix is Some && p_owner(self.perm(h)) == UNCOUNTED,
            forall|i: int| #[trigger]
                p_scanned(self.perm(h), i) ==> p_init(self.perm(h), i) && self.word_ok(
                    p_word(self.perm(h), i),
                ) && (p_word(self.perm(h), i).is_ptr_spec() ==> self.in_region(
                    p_word(self.perm(h), i).bits_spec(),
                )),
    {
        let r = self.region();
        r.lemma_block(h, self.checked());
        assert forall|i: int| #[trigger] p_scanned(self.perm(h), i) implies p_init(self.perm(h), i)
            && self.word_ok(p_word(self.perm(h), i)) && (p_word(self.perm(h), i).is_ptr_spec()
            ==> self.in_region(p_word(self.perm(h), i).bits_spec())) by {
            let w = p_word(self.perm(h), i);
            assert(r.word_ok(w));
            if w.is_ptr_spec() {
                self.lemma_apart(w.bits_spec());
            }
        }
    }

    /// 安全の条件を満たすヒープのブロックの形。
    pub(crate) proof fn lemma_safe_at(&self, h: usize)
        requires
            self.safe_wf(),
            self.dom().contains(h),
        ensures
            self.safe_block_wf(h),
            self.pages.carved_block(h),
            self.block(h)@.addr == h,
            h % 8 == 0,
            h >= 8,
            h + 8 * (1 + self.cap(h)) <= usize::MAX,
    {
        reveal(Core::safe_blocks_wf);
        assert(self.safe_block_wf(h));
        self.pages.lemma_block_bounds(h);
    }
}

} // verus!
