//! 実行文脈 `Ctx` (docs/spec/lir.md の「実行の文脈」)。ヒープと、今の `evv` と、yield の状態を持ち、基本操作
//! (docs/spec/lir.md の「基本操作」) をメソッドにする。ヒープの spec の関数の中を開けるため、`heap` の子に置く。

use std::sync::Arc;

use vstd::prelude::*;

use super::Heap;
#[cfg(verus_only)]
use super::{Core, chain_err};
use crate::control::{Control, Stop};
use crate::error::HeapError;
use crate::mode::Mode;
use crate::word::Word;
#[cfg(verus_only)]
use crate::word::lemma_unit_bits;

verus! {

/// 検査モードだけで数える仕事の回数。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct CtxStats {
    pub evidence_steps: u64,
    pub yields: u64,
    pub yield_frames: u64,
}

/// yield の状態。`target` が `()` なら yield していない。Rust の enum は配置が決まらないので、平らな記録にして、
/// ネイティブのコードが決まった位置の語を1回比べれば yield 中かを読めるようにする。
#[repr(C)]
pub(crate) struct YieldState {
    /// 宛先の evidence。
    pub(crate) target: Word,
    /// 節の本体のクロージャ。
    pub(crate) clause: Word,
    /// 組み立て中の継続 κ。
    pub(crate) cont: Word,
    /// 中断か。
    pub(crate) abort: bool,
}

impl YieldState {
    pub(crate) open spec fn wf(&self) -> bool {
        self.target.bits_spec() == 1 ==> {
            &&& self.clause.bits_spec() == 1
            &&& self.cont.bits_spec() == 1
            &&& !self.abort
        }
    }

    fn none() -> (r: YieldState)
        ensures
            r.target.bits_spec() == 1,
            r.wf(),
    {
        YieldState { target: Word::UNIT, clause: Word::UNIT, cont: Word::UNIT, abort: false }
    }
}

/// 実行文脈。VM はこれだけを持ち回す。`evv` と yield の状態の語は、`Ctx` が持つ数えた参照である。`Ctx` を落としても
/// これらの語には触れず、ヒープと一緒に消える。
pub struct Ctx<M: Mode> {
    heap: Heap<M>,
    evv: Word,
    yielding: YieldState,
    /// 安全点の確かめがヒープの中をたどらずに読めるよう、ヒープとは別に持つ。
    control: Arc<Control>,
    max_depth: usize,
    stats: CtxStats,
}

impl<M: Mode> Ctx<M> {
    pub closed spec fn heap_spec(&self) -> Heap<M> {
        self.heap
    }

    pub closed spec fn evv_spec(&self) -> Word {
        self.evv
    }

    /// yield の状態。宛先、節の本体、κ、中断か、の組である。
    pub closed spec fn yield_spec(&self) -> (Word, Word, Word, bool) {
        (self.yielding.target, self.yielding.clause, self.yielding.cont, self.yielding.abort)
    }

    pub open spec fn is_yielding(&self) -> bool {
        self.yield_spec().0.bits_spec() != 1
    }

    /// κ の欄が空。関数 0 の値も `()` と同じ語なので、空に見える。
    pub open spec fn cont_empty(&self) -> bool {
        self.yield_spec().2.bits_spec() == 1
    }

    /// `evv` が空で、yield していない。
    pub open spec fn finished(&self) -> bool {
        self.evv_spec().bits_spec() == 1 && !self.is_yielding()
    }

    /// `w` は `evv` のリストとして正しい。
    pub closed spec fn evv_wf(&self, w: Word) -> bool {
        w.bits_spec() == 1
    }

    /// `w` は即値か、このヒープの生きている物体を指し、参照を1つ持っていて、`decref` で手放せる。
    pub closed spec fn holds(&self, w: Word) -> bool {
        &&& self.heap.holds(w)
        &&& self.heap.owned(w)
        &&& w.is_ptr_spec() && self.heap.rc_value(w) < 0 ==> !self.heap.lin_of(w)
    }

    /// モデルが合っていて、`Ctx` の持つ語を `Ctx` が持っている。速いモードの基本操作の前提であり、検査モードでは、
    /// これが成り立てば基本操作は `Ok` を返す。
    pub closed spec fn held_ok(&self) -> bool {
        &&& self.heap.consistent()
        &&& self.heap.settleable()
        &&& self.evv_wf(self.evv)
        &&& self.holds(self.evv)
        &&& self.holds(self.yielding.target)
        &&& self.holds(self.yielding.clause)
        &&& self.holds(self.yielding.cont)
    }

    /// 不変条件。検査モードでは安全の条件で、`Err` の後も保つ。速いモードでは `held_ok` も含む。
    pub closed spec fn wf(&self) -> bool {
        &&& self.heap.wf()
        &&& self.heap.safe_settleable()
        &&& self.yielding.wf()
        &&& self.heap.word_in(self.evv)
        &&& self.heap.word_in(self.yielding.target)
        &&& self.heap.word_in(self.yielding.clause)
        &&& self.heap.word_in(self.yielding.cont)
        &&& !M::CHECKED ==> self.held_ok()
    }

    /// `evv` が空で、yield していない文脈。ページはセッションの `control` に数える。
    pub fn new(control: Arc<Control>) -> (r: Ctx<M>)
        ensures
            r.wf(),
            r.held_ok(),
            r.finished(),
    {
        let max_depth = control.limits().max_depth;
        let heap = Heap::with_control(control.clone());
        proof {
            lemma_unit_bits();
        }
        Ctx {
            heap,
            evv: Word::UNIT,
            yielding: YieldState::none(),
            control,
            max_depth,
            stats: CtxStats { evidence_steps: 0, yields: 0, yield_frames: 0 },
        }
    }

    pub fn heap(&self) -> (r: &Heap<M>)
        ensures
            *r == self.heap_spec(),
    {
        &self.heap
    }

    /// # Safety
    /// ヒープを入れ替えない。`Ctx` が持つ語 (`evv` と yield の状態) を手放したり書き換えたりしない。`Ctx` のメソッドを
    /// 次に呼ぶ前に、作っている物体を書き終える。
    pub unsafe fn heap_mut(&mut self) -> &mut Heap<M> {
        &mut self.heap
    }

    /// VM がリークを確かめる前に呼ぶ (docs/spec/lir.md の「`evv` の不変条件」)。
    pub fn check_finished(&self) -> (r: bool)
        ensures
            r == self.finished(),
    {
        self.evv.bits() == 1 && self.yielding.target.bits() == 1
    }

    pub fn stats(&self) -> CtxStats {
        self.stats
    }

    /// yield 中か中断中なら 1、そうでなければ 0。
    pub fn yielding(&self) -> (r: i64)
        ensures
            r == if self.is_yielding() {
                1i64
            } else {
                0i64
            },
    {
        if self.yielding.target.bits() == 1 {
            0
        } else {
            1
        }
    }

    /// 0: yield していない、1: `ev` 宛ての yield、2: `ev` 宛ての中断、3: ほかの宛先の yield か中断。宛先は語の値で
    /// 比べる。yield の状態が宛先の参照を持つので、比べる間に番地が使い回されることはない。
    pub fn yield_status(&self, ev: Word) -> (r: i64)
        ensures
            r == if !self.is_yielding() {
                0i64
            } else if self.yield_spec().0.bits_spec() != ev.bits_spec() {
                3i64
            } else if self.yield_spec().3 {
                2i64
            } else {
                1i64
            },
    {
        let target = self.yielding.target.bits();
        if target == 1 {
            0
        } else if target != ev.bits() {
            3
        } else if self.yielding.abort {
            2
        } else {
            1
        }
    }

    /// yield か中断を始める。`ev`、`clause`、`k` の参照を `Ctx` が持つ。
    fn start_yield(&mut self, ev: Word, clause: Word, k: Word, abort: bool) -> (r: Result<
        (),
        HeapError,
    >)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(ev),
            old(self).heap_spec().word_in(clause),
            old(self).heap_spec().word_in(k),
            !M::CHECKED ==> !old(self).is_yielding() && ev.is_ptr_spec() && old(self).holds(ev)
                && old(self).holds(clause) && old(self).holds(k),
        ensures
            final(self).wf(),
            r is Err <==> M::CHECKED && (old(self).is_yielding() || !ev.is_ptr_spec()),
            r is Err ==> r == Err::<(), HeapError>(HeapError::YieldMisuse) && *final(self) == *old(
                self,
            ),
            r is Ok ==> final(self).yield_spec() == (ev, clause, k, abort),
            final(self).heap_spec() == old(self).heap_spec(),
            final(self).evv_spec() == old(self).evv_spec(),
    {
        if M::CHECKED && (self.yielding.target.bits() != 1 || !ev.is_ptr()) {
            return Err(HeapError::YieldMisuse);
        }
        proof {
            crate::word::lemma_imm_and_ptr_are_disjoint(ev.bits_spec());
            lemma_unit_bits();
        }
        self.yielding = YieldState { target: ev, clause, cont: k, abort };
        if M::CHECKED {
            self.stats.yields = self.stats.yields.wrapping_add(1);
        }
        Ok(())
    }

    /// yield を始める。`ev` は宛先の evidence、`clause` は節の本体、`k` は最初の κ で、どれも消費する。
    ///
    /// # Safety
    /// 3つの語は即値か、このヒープか付けた塊の生きている物体を指し、呼ぶ側がその参照を1つずつ持つ。`ev` は物体を指す。
    /// yield していない (検査モードは `YieldMisuse` を返す)。
    pub unsafe fn yield_to(&mut self, ev: Word, clause: Word, k: Word) -> (r: Result<(), HeapError>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(ev),
            old(self).heap_spec().word_in(clause),
            old(self).heap_spec().word_in(k),
            !M::CHECKED ==> !old(self).is_yielding() && ev.is_ptr_spec() && old(self).holds(ev)
                && old(self).holds(clause) && old(self).holds(k),
        ensures
            final(self).wf(),
            r is Err <==> M::CHECKED && (old(self).is_yielding() || !ev.is_ptr_spec()),
            r is Err ==> r == Err::<(), HeapError>(HeapError::YieldMisuse) && *final(self) == *old(
                self,
            ),
            r is Ok ==> final(self).yield_spec() == (ev, clause, k, false),
            final(self).heap_spec() == old(self).heap_spec(),
            final(self).evv_spec() == old(self).evv_spec(),
    {
        self.start_yield(ev, clause, k, false)
    }

    /// 中断を始める。引数と前提は `yield_to` と同じである。
    ///
    /// # Safety
    /// `yield_to` と同じ。
    pub unsafe fn yield_abort(&mut self, ev: Word, clause: Word, k: Word) -> (r: Result<
        (),
        HeapError,
    >)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(ev),
            old(self).heap_spec().word_in(clause),
            old(self).heap_spec().word_in(k),
            !M::CHECKED ==> !old(self).is_yielding() && ev.is_ptr_spec() && old(self).holds(ev)
                && old(self).holds(clause) && old(self).holds(k),
        ensures
            final(self).wf(),
            r is Err <==> M::CHECKED && (old(self).is_yielding() || !ev.is_ptr_spec()),
            r is Err ==> r == Err::<(), HeapError>(HeapError::YieldMisuse) && *final(self) == *old(
                self,
            ),
            r is Ok ==> final(self).yield_spec() == (ev, clause, k, true),
            final(self).heap_spec() == old(self).heap_spec(),
            final(self).evv_spec() == old(self).evv_spec(),
    {
        self.start_yield(ev, clause, k, true)
    }

    /// κ を取り出し、欄を `()` にする。κ の参照は呼ぶ側に移る。ヒープに触れないので安全な関数にする。yield して
    /// いなければ、検査モードは `YieldMisuse` を返し、速いモードは空の欄の `()` を返す。
    pub fn yield_cont_take(&mut self) -> (r: Result<Word, HeapError>)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            r is Err <==> M::CHECKED && !old(self).is_yielding(),
            r is Err ==> r == Err::<Word, HeapError>(HeapError::YieldMisuse) && *final(self)
                == *old(self),
            r matches Ok(k) ==> {
                &&& k == old(self).yield_spec().2
                &&& final(self).yield_spec().0 == old(self).yield_spec().0
                &&& final(self).yield_spec().1 == old(self).yield_spec().1
                &&& final(self).cont_empty()
                &&& final(self).yield_spec().3 == old(self).yield_spec().3
            },
            final(self).heap_spec() == old(self).heap_spec(),
            final(self).evv_spec() == old(self).evv_spec(),
    {
        if M::CHECKED && self.yielding.target.bits() == 1 {
            return Err(HeapError::YieldMisuse);
        }
        let k = self.yielding.cont;
        self.yielding.cont = Word::UNIT;
        proof {
            lemma_unit_bits();
        }
        Ok(k)
    }

    /// κ を入れる。`k` は消費する。
    ///
    /// # Safety
    /// `k` は即値か、このヒープか付けた塊の生きている物体を指し、呼ぶ側がその参照を1つ持つ。yield 中で、κ の欄が空で
    /// ある (検査モードは `YieldMisuse` を返す)。
    pub unsafe fn yield_cont_set(&mut self, k: Word) -> (r: Result<(), HeapError>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(k),
            !M::CHECKED ==> old(self).is_yielding() && old(self).cont_empty() && old(self).holds(k),
        ensures
            final(self).wf(),
            r is Err <==> M::CHECKED && !(old(self).is_yielding() && old(self).cont_empty()),
            r is Err ==> r == Err::<(), HeapError>(HeapError::YieldMisuse) && *final(self) == *old(
                self,
            ),
            r is Ok ==> final(self).yield_spec() == (
                old(self).yield_spec().0,
                old(self).yield_spec().1,
                k,
                old(self).yield_spec().3,
            ),
            final(self).heap_spec() == old(self).heap_spec(),
            final(self).evv_spec() == old(self).evv_spec(),
    {
        if M::CHECKED && (self.yielding.target.bits() == 1 || self.yielding.cont.bits() != 1) {
            return Err(HeapError::YieldMisuse);
        }
        self.yielding.cont = k;
        if M::CHECKED {
            self.stats.yield_frames = self.stats.yield_frames.wrapping_add(1);
        }
        Ok(())
    }

    /// yield を終える。節の本体を返し、宛先の参照を手放し、yield の状態をなしに戻す。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。yield 中で、κ の欄が空である (検査モードは `YieldMisuse` を返す)。検査モードで宛先を手放す連鎖が誤りを見つけたら、
    /// `Err` を返した後は `Ctx` を落とすことしかできない (docs/spec/object-model.md の「検査モードの約束」)。
    pub unsafe fn yield_finish(&mut self) -> (r: Result<Word, HeapError>)
        requires
            old(self).wf(),
            !M::CHECKED ==> old(self).is_yielding() && old(self).cont_empty(),
        ensures
            final(self).wf(),
            !M::CHECKED ==> r is Ok,
            M::CHECKED && !(old(self).is_yielding() && old(self).cont_empty()) ==> r == Err::<
                Word,
                HeapError,
            >(HeapError::YieldMisuse) && *final(self) == *old(self),
            old(self).is_yielding() && old(self).cont_empty() ==> {
                &&& !final(self).is_yielding()
                &&& final(self).evv_spec() == old(self).evv_spec()
                &&& final(self).heap_spec().kept_live(old(self).heap_spec())
                &&& old(self).held_ok() ==> r is Ok || chain_err(r)
                &&& old(self).held_ok() && r is Ok ==> r == Ok::<Word, HeapError>(
                    old(self).yield_spec().1,
                ) && final(self).heap_spec().evolved(
                    old(self).heap_spec(),
                    old(self).yield_spec().0,
                    -1,
                )
            },
    {
        if M::CHECKED && (self.yielding.target.bits() == 1 || self.yielding.cont.bits() != 1) {
            return Err(HeapError::YieldMisuse);
        }
        let target = self.yielding.target;
        let clause = self.yielding.clause;
        self.yielding = YieldState::none();
        proof {
            lemma_unit_bits();
        }
        let r = unsafe { self.heap.decref(target) };
        proof {
            let s0 = old(self);
            assert(s0.heap.word_in(s0.evv));
            if !M::CHECKED {
                // 速いモードの `evv` は空のリストである
                self.lemma_unit_holds(self.evv);
            }
            self.lemma_unit_holds(self.yielding.target);
            self.lemma_unit_holds(self.yielding.clause);
            self.lemma_unit_holds(self.yielding.cont);
        }
        match r {
            Ok(()) => Ok(clause),
            Err(e) => Err(e),
        }
    }

    /// 今の `evv` を `dup` して返す。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている (`Ctx` の持つ語は生きている)。
    pub unsafe fn evv_get(&mut self) -> (r: Result<Word, HeapError>)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            !M::CHECKED ==> r is Ok,
            r matches Ok(w) ==> w == old(self).evv_spec(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            old(self).held_ok() ==> r is Ok && final(self).heap_spec().evolved(
                old(self).heap_spec(),
                old(self).evv_spec(),
                1,
            ) && final(self).heap_spec().bumped(old(self).heap_spec(), old(self).evv_spec()),
    {
        let w = self.evv;
        proof {
            lemma_unit_bits();
        }
        let r = unsafe { self.heap.dup(w) };
        proof {
            if !M::CHECKED {
                self.lemma_held_bumped(*old(self));
            } else {
                self.lemma_words_kept(*old(self));
            }
        }
        match r {
            Ok(()) => Ok(w),
            Err(e) => Err(e),
        }
    }

    /// `evv` を `l` に替え、元の `evv` を手放す。`l` は消費する。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。`l` は即値か、このヒープか付けた塊の生きている物体を指し、呼ぶ側がその参照を1つ持つ。速いモードでは、`l` は
    /// `evv` のリストとして正しい。検査モードで元の `evv` を手放す連鎖が誤りを見つけたら、`Err` を返した後は `Ctx` を
    /// 落とすことしかできない (docs/spec/object-model.md の「検査モードの約束」)。
    pub unsafe fn evv_set(&mut self, l: Word) -> (r: Result<(), HeapError>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(l),
            !M::CHECKED ==> old(self).evv_wf(l) && old(self).holds(l),
        ensures
            final(self).wf(),
            !M::CHECKED ==> r is Ok,
            final(self).evv_spec() == l,
            final(self).yield_spec() == old(self).yield_spec(),
            final(self).heap_spec().kept_live(old(self).heap_spec()),
            old(self).held_ok() && old(self).evv_wf(l) && old(self).holds(l) ==> r is Ok
                || chain_err(r),
            old(self).held_ok() && old(self).evv_wf(l) && old(self).holds(l) && r is Ok
                ==> final(self).heap_spec().evolved(
                old(self).heap_spec(),
                old(self).evv_spec(),
                -1,
            ),
    {
        let old_evv = self.evv;
        self.evv = l;
        proof {
            lemma_unit_bits();
        }
        let r = unsafe { self.heap.decref(old_evv) };
        proof {
            if !M::CHECKED {
                self.lemma_held_settled(*old(self));
            } else {
                assert(old(self).heap.word_in(l));
                self.lemma_words_kept(*old(self));
            }
        }
        r
    }
}

impl<M: Mode> Ctx<M> {
    proof fn lemma_unit_holds(&self, w: Word)
        requires
            w.bits_spec() == 1,
        ensures
            self.holds(w),
            self.heap.word_in(w),
    {
        lemma_unit_bits();
    }

    /// 検査モードの操作は語を `word_in` のまま保つので、`Ctx` の持つ語も `word_in` のままである。
    proof fn lemma_words_kept(&self, old: Ctx<M>)
        requires
            self.heap.words_kept(old.heap),
            old.heap.word_in(old.evv),
            old.heap.word_in(old.yielding.target),
            old.heap.word_in(old.yielding.clause),
            old.heap.word_in(old.yielding.cont),
            self.yielding == old.yielding,
        ensures
            self.heap.word_in(old.evv),
            self.heap.word_in(self.yielding.target),
            self.heap.word_in(self.yielding.clause),
            self.heap.word_in(self.yielding.cont),
    {
    }

    /// `dup` の後も、`Ctx` の持つ語は持たれたままである。`bumped` は `dup` した物体の rc と数のほかを変えない。
    proof fn lemma_holds_bumped(&self, old: Ctx<M>, w: Word, x: Word)
        requires
            old.holds(x),
            old.heap.word_in(x),
            old.heap.consistent(),
            self.heap.bumped(old.heap, w),
            self.heap.settled(),
        ensures
            self.holds(x),
            self.heap.word_in(x),
    {
        let s = self.heap.core;
        let v = old.heap.core.settled_view();
        assert(s.settled_view() == s);
        if x.is_ptr_spec() {
            let b = x.bits_spec();
            let h = w.bits_spec();
            assert(s.model@.edges == v.model@.edges);
            if !v.dom().contains(h) {
                reveal(Core::blocks_wf);
                assert(s.model@.refs =~= v.model@.refs);
            }
            if b != h {
                assert(s.perm(b) == v.perm(b));
            }
            assert(s.ext(b) >= v.ext(b));
        }
    }

    proof fn lemma_held_bumped(&self, old: Ctx<M>)
        requires
            !M::CHECKED,
            old.wf(),
            self.heap.wf(),
            self.heap.consistent(),
            self.heap.settled(),
            self.heap.bumped(old.heap, old.evv),
            self.evv == old.evv,
            self.yielding == old.yielding,
        ensures
            self.wf(),
    {
        self.lemma_holds_bumped(old, old.evv, self.evv);
        self.lemma_holds_bumped(old, old.evv, self.yielding.target);
        self.lemma_holds_bumped(old, old.evv, self.yielding.clause);
        self.lemma_holds_bumped(old, old.evv, self.yielding.cont);
    }

    /// ヒープが構築中の物体を生きている物体にしただけなら、`Ctx` の持つ語は持たれたままである。
    proof fn lemma_held_settled(&self, old: Ctx<M>)
        requires
            !M::CHECKED,
            old.wf(),
            old.evv_wf(self.evv),
            old.holds(self.evv),
            old.heap.word_in(self.evv),
            self.heap.wf(),
            self.heap.consistent(),
            self.heap.settled(),
            self.heap == old.heap.settled_heap(),
            self.yielding == old.yielding,
        ensures
            self.wf(),
    {
    }
}

} // verus!
// 安全点の確かめは何も約束しないので、`verus!` の外に書く (docs/spec/lir.md の「実行の文脈」)。止める語は「止めよ」の
// ビットだけを伝えるので、Relaxed で読む
impl<M: Mode> Ctx<M> {
    /// 安全点の確かめ。`depth` は VM のタスクのスタックの語の数である。
    #[inline]
    pub fn poll(&self, depth: usize) -> Result<(), Stop> {
        let bits = self.control.stop_bits();
        if depth > self.max_depth || bits != 0 {
            return Err(stop_reason(bits));
        }
        Ok(())
    }
}

/// 止める理由。割り込みを確保量より先に、確保量を深さより先に報告する。ビットは速い道で読んだ値を受け、読み直さない。
/// 読み直すと、その間にほかのスレッドが `clear` したとき、深さの上限を超えていなくても `DepthLimit` を返すためである。
#[cold]
fn stop_reason(bits: u32) -> Stop {
    if bits & Control::INTERRUPT != 0 {
        Stop::Interrupted
    } else if bits & Control::MEMORY != 0 {
        Stop::MemoryLimit
    } else {
        Stop::DepthLimit
    }
}
