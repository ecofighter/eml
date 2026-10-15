//! セッションの根 (docs/spec/object-model.md の「実行文脈 `Ctx`」)。
//!
//! 根は REPL の `let` の束縛の値を入力をまたいで持つ。根の語は、VM のスタックの語と同じく、呼ぶ側 (VM) が参照を
//! 持つと約束する数えた参照である。根を `Ctx` の不変条件 (`wf`、`held_ok`) に入れないのは、根を読み書きしない
//! 基本操作の証明を変えずに済ませるためである。そのため `root_get` は、根の語が生きていることを前提で受ける。

#[cfg(verus_only)]
use super::proof::*;
use super::*;

verus! {

impl<M: Mode> Ctx<M> {
    /// 根 `index` の語。空か表の外なら `None`。
    pub closed spec fn root_spec(&self, index: int) -> Option<Word> {
        if 0 <= index < self.roots@.len() {
            self.roots@[index]
        } else {
            None
        }
    }

    /// 根の表の長さ。
    pub closed spec fn root_len(&self) -> int {
        self.roots@.len() as int
    }

    /// 根の表の読み取りの中身。回復 (`recover.rs`) が根の表を写すのに使う。
    pub(super) proof fn lemma_root_spec(&self)
        ensures
            self.root_len() == self.roots@.len(),
            forall|j: int| #[trigger]
                self.root_spec(j) == if 0 <= j < self.roots@.len() {
                    self.roots@[j]
                } else {
                    None
                },
    {
    }

    /// 根の表だけを変えた文脈は、不変条件を保つ。`wf` は根を見ないが、`holds_args` の量化子は文脈ごとの `need` で
    /// 引かれるので、新しい文脈の `need` を古い文脈の `need` につなぐ。
    proof fn lemma_roots_frame(&self, old: Ctx<M>)
        requires
            old.wf(),
            self.heap == old.heap,
            self.evv == old.evv,
            self.yielding == old.yielding,
            self.max_evv == old.max_evv,
        ensures
            self.wf(),
    {
        if !M::CHECKED {
            lemma_moved_refl(self.heap);
            assert forall|b: usize| #[trigger] self.need(seq![], b) > 0 implies old.need(seq![], b)
                + 0 >= self.need(seq![], b) by {
                lemma_points_in(seq![], b);
            }
            self.lemma_holds_moved(old, seq![], seq![], |b: usize| 0int, Set::empty());
        }
    }

    /// 根 `index` を `dup` して返す。空の根と表の外の根は、どちらのモードでも `RootMisuse` にする。正しいプログラムでは
    /// 起きないが、確かめは比較1回で済み、速いモードで表の外を読まないためである。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。根 `index` の語は即値か、このヒープか付けた塊の生きている物体を指し、根がその
    /// 参照を1つ持つ。物体は `lin` ビットを持たず、rc が正なら持ち主は今の worker である。
    pub unsafe fn root_get(&mut self, index: u32) -> (r: Result<Word, HeapError>)
        requires
            old(self).wf(),
            old(self).root_spec(index as int) matches Some(w) ==> {
                &&& old(self).heap_spec().word_in(w)
                &&& !M::CHECKED ==> old(self).heap_spec().dup_ok(w)
            },
        ensures
            final(self).wf(),
            old(self).root_spec(index as int) is None ==> r == Err::<Word, HeapError>(
                HeapError::RootMisuse,
            ) && *final(self) == *old(self),
            r matches Ok(w) ==> old(self).root_spec(index as int) == Some(w),
            !M::CHECKED && old(self).root_spec(index as int) is Some ==> r is Ok,
            forall|j: int| #[trigger] final(self).root_spec(j) == old(self).root_spec(j),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
    {
        let i = index as usize;
        if i >= self.roots.len() {
            return Err(HeapError::RootMisuse);
        }
        let w = match self.roots[i] {
            Some(w) => w,
            None => return Err(HeapError::RootMisuse),
        };
        let r = unsafe { self.heap.dup(w) };
        proof {
            let s0 = *old(self);
            if M::CHECKED {
                self.lemma_words_kept(s0);
            }
            if !M::CHECKED {
                lemma_dup_moved(self.heap, s0.heap, w);
                let d = |b: usize| points(w, b);
                assert forall|b: usize| #[trigger] self.need(seq![], b) > 0 implies s0.need(
                    seq![],
                    b,
                ) + d(b) >= self.need(seq![], b) by {
                    lemma_points_in(seq![], b);
                }
                self.lemma_holds_moved(s0, seq![], seq![], d, Set::empty());
                self.lemma_evv_kept(s0, d, Set::empty());
            }
        }
        match r {
            Ok(()) => Ok(w),
            Err(e) => Err(e),
        }
    }

    /// 根 `index` に `value` を置く。`value` の参照は根に移る。表は `index` まで伸び、間の根は空のまま残る。空でない根
    /// には置かず、`RootMisuse` を返して `value` を呼ぶ側に残す。ヒープに触れないので、`Ctx` の不変条件は変わらない。
    ///
    /// # Safety
    /// `value` は即値か、このヒープか付けた塊の生きている物体を指し、呼ぶ側がその参照を1つ持つ。
    pub unsafe fn root_set(&mut self, index: u32, value: Word) -> (r: Result<(), HeapError>)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            r is Err <==> old(self).root_spec(index as int) is Some,
            r is Err ==> r == Err::<(), HeapError>(HeapError::RootMisuse) && *final(self) == *old(
                self,
            ),
            r is Ok ==> final(self).root_spec(index as int) == Some(value),
            r is Ok ==> forall|j: int| #[trigger]
                final(self).root_spec(j) == if j == index as int {
                    Some(value)
                } else {
                    old(self).root_spec(j)
                },
            final(self).heap_spec() == old(self).heap_spec(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
    {
        let i = index as usize;
        if i < self.roots.len() && self.roots[i].is_some() {
            return Err(HeapError::RootMisuse);
        }
        while self.roots.len() <= i
            invariant
                self.heap == old(self).heap,
                self.evv == old(self).evv,
                self.yielding == old(self).yielding,
                self.max_evv == old(self).max_evv,
                old(self).roots@.len() <= self.roots@.len(),
                old(self).roots@.len() < self.roots@.len() ==> self.roots@.len() <= i + 1,
                forall|j: int|
                    0 <= j < old(self).roots@.len() ==> self.roots@[j] == old(self).roots@[j],
                forall|j: int|
                    old(self).roots@.len() <= j < self.roots@.len() ==> self.roots@[j] is None,
            decreases i + 1 - self.roots@.len(),
        {
            self.roots.push(None);
        }
        self.roots.set(i, Some(value));
        proof {
            self.lemma_roots_frame(*old(self));
        }
        Ok(())
    }

    /// 根をすべて取り出し、表を空にする。根の参照は呼ぶ側に移る。ヒープに触れないので安全な関数にする。
    pub fn take_roots(&mut self) -> (r: Vec<Option<Word>>)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            r@.len() == old(self).root_len(),
            forall|j: int| 0 <= j < r@.len() ==> #[trigger] r@[j] == old(self).root_spec(j),
            forall|j: int| #[trigger] final(self).root_spec(j) is None,
            final(self).heap_spec() == old(self).heap_spec(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
    {
        let mut taken = Vec::new();
        std::mem::swap(&mut taken, &mut self.roots);
        proof {
            self.lemma_roots_frame(*old(self));
        }
        taken
    }
}

} // verus!
