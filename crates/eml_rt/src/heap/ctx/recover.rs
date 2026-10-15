//! 実行時エラーの後の回復 (docs/spec/object-model.md の「回復」)。
//!
//! 根から届く物体を新しいヒープに写し、古いヒープを丸ごと捨てる。捨てたスタックと evv と yield の語、token の箱は
//! 古いヒープと一緒に消え、届かない `Foreign` の物体は古いヒープの破棄で閉じ、ページはすべて `Control` に戻る。

#[cfg(verus_only)]
use super::proof::*;
use super::*;

verus! {

impl<M: Mode> Ctx<M> {
    /// 根の表の語は、それぞれヒープの参照を1つ持つ。
    pub closed spec fn roots_held(&self) -> bool {
        self.heap.roots_counted(self.roots@)
    }

    /// 続けられる実行時エラーの後、根から届く物体を新しいヒープに写し、古いヒープを捨てる。evv と yield の状態は空に
    /// 戻す。写せない物体 (`Foreign`、`lin` ビットの物体、輪、共有の物体、ダミーのほかの不死の物体、検査モードの
    /// 生存でない物体) に届けば、古いヒープと根を残したまま誤りを返す。そのときヒープは構築中の物体を書き終えた形に
    /// なるだけで、物体は変わらない。
    ///
    /// # Safety
    /// 根の語は即値か、このヒープか付けた塊の生きている物体を指し、根がその参照を1つ持つ。
    pub unsafe fn recover(&mut self) -> (r: Result<(), HeapError>)
        requires
            old(self).wf(),
            forall|j: int| #[trigger]
                old(self).root_spec(j) matches Some(w) ==> old(self).heap_spec().word_in(w) && (
                !M::CHECKED ==> old(self).heap_spec().readable(w)),
        ensures
            final(self).wf(),
            final(self).root_len() == old(self).root_len(),
            forall|j: int|
                (#[trigger] final(self).root_spec(j) is Some) == (old(self).root_spec(j) is Some),
            r is Err ==> {
                &&& final(self).heap_spec() == old(self).heap_spec().settled_heap()
                &&& forall|j: int| #[trigger] final(self).root_spec(j) == old(self).root_spec(j)
                &&& final(self).evv_spec() == old(self).evv_spec()
                &&& final(self).yield_spec() == old(self).yield_spec()
            },
            r is Ok ==> {
                &&& final(self).finished()
                &&& forall|j: int| #[trigger]
                    final(self).root_spec(j) matches Some(w) ==> final(self).heap_spec().word_in(w)
                        && (!M::CHECKED ==> final(self).heap_spec().readable(w))
                &&& !M::CHECKED ==> final(self).roots_held()
            },
    {
        let ghost s0 = *old(self);
        proof {
            s0.lemma_root_spec();
        }
        self.heap.settle();
        proof {
            assert forall|j: int| 0 <= j < self.roots@.len() implies (
            #[trigger] self.roots@[j] matches Some(w) ==> self.heap.source_word(w)) by {
                if self.roots@[j] is Some {
                    assert(s0.root_spec(j) == self.roots@[j]);
                }
            }
        }
        let mut fresh = self.heap.fresh();
        match self.heap.copy_roots(&mut fresh, self.roots.as_slice()) {
            Ok(out) => {
                // 入れ替えた古いヒープは、この関数を出るときに落ちる。届かない `Foreign` の物体の破棄処理を呼び、ページを
                // `Control` に戻す
                std::mem::swap(&mut self.heap, &mut fresh);
                self.roots = out;
                self.evv = Word::UNIT;
                self.yielding = YieldState::none();
                proof {
                    lemma_unit_bits();
                    lemma_points_in(seq![], 0);
                    self.lemma_unit_listed(self.evv);
                    assert forall|b: usize| #[trigger] self.need(seq![], b) > 0 implies false by {
                        lemma_points_in(seq![], b);
                    }
                    self.lemma_root_spec();
                }
                Ok(())
            },
            Err(e) => {
                proof {
                    self.lemma_root_spec();
                    // 古いヒープは、構築中の物体を書き終えた形になっただけである
                    let z = |b: usize| 0int;
                    let none = Set::<usize>::empty();
                    lemma_settle_moved(self.heap, s0.heap);
                    assert forall|w: Word|
                        #![trigger s0.heap.word_in(w)]
                        #![trigger self.heap.word_in(w)]
                        s0.heap.word_in(w) implies self.heap.word_in(w) by {}
                    self.lemma_words_kept(s0);
                    if !M::CHECKED {
                        assert forall|b: usize| #[trigger] self.need(seq![], b) > 0 implies s0.need(
                            seq![],
                            b,
                        ) + z(b) >= self.need(seq![], b) by {
                            lemma_points_in(seq![], b);
                        }
                        self.lemma_holds_moved(s0, seq![], seq![], z, none);
                        self.lemma_evv_kept(s0, z, none);
                    }
                }
                Err(e)
            },
        }
    }
}

} // verus!
