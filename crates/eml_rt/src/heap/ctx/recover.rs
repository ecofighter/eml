//! 実行時エラーの後の回復 (docs/spec/object-model.md の「回復」)。
//!
//! 根から届く物体を新しいヒープに写し、古いヒープを丸ごと捨てる。捨てたスタックと evv と yield の語、token の箱は
//! 古いヒープと一緒に消え、届かない `Foreign` の物体は古いヒープの破棄で閉じ、ページはすべて `Control` に戻る。

#[cfg(verus_only)]
use super::proof::*;
use super::*;
use crate::remote::sink_pushing;

verus! {

impl<M: Mode> Ctx<M> {
    /// 根の表の語は、それぞれヒープの参照を1つ持つ。
    pub closed spec fn roots_held(&self) -> bool {
        self.heap.roots_counted(self.roots@)
    }

    /// 続けられる実行時エラーの後、根から届く物体を新しいヒープに写し、古いヒープを捨てる。先にほかのスレッドから
    /// 受けた解放を取り込む。evv と yield の状態は空に戻す。共有の物体は局所の物体として写す。同じ `Control` のほかの
    /// ヒープのページの物体も写す。写せない物体 (`Foreign`、`lin` ビットの物体、輪、ダミーのほかの不死の物体、
    /// 検査モードの生存でない物体) に届けば、古いヒープと根を残したまま誤りを返す。検査モードで、登録していない
    /// ページを指す語に届けば `UnknownPage` を、取り込んだ後も解放を積んでいる途中のスレッドがあれば `NotQuiescent` を
    /// 返す。誤りのときヒープは、受けた解放を取り込み、構築中の物体を書き終えた形になるだけである。
    ///
    /// # Safety
    /// 根の語は即値か、このヒープか同じ `Control` のほかのヒープか付けた塊の生きている物体を指し、根がその参照を1つ
    /// 持つ。ほかのヒープは、この呼び出しが戻るまで生きていて、どのスレッドも触れない (docs/spec/object-model.md の
    /// 「回復」)。
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
                &&& final(self).heap_spec().reclaimed(
                    old(self).heap_spec().settled_heap(),
                    old(self).kept_set(),
                )
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
        let ghost c1 = *self;
        proof {
            assert forall|j: int| 0 <= j < self.roots@.len() implies (
            #[trigger] self.roots@[j] matches Some(w) ==> self.heap.source_word(w)) by {
                if self.roots@[j] is Some {
                    assert(s0.root_spec(j) == self.roots@[j]);
                }
            }
        }
        // ページを歩くほかの操作と同じく、写す前にほかのスレッドから受けた解放を取り込む
        // (docs/spec/object-model.md の「別のスレッドからの解放」)。根と `Ctx` の持つ語は参照を持つので、その物体を
        // 空けたことにはしない
        let ghost held = s0.kept_set();
        self.heap.reclaim_held(Ghost(held));
        proof {
            assert(s0.held_set().subset_of(held));
            self.lemma_reclaimed_ctx(c1, s0, held);
            let r = self.heap.lemma_reclaimed(c1.heap, held);
            assert forall|j: int| 0 <= j < self.roots@.len() implies (
            #[trigger] self.roots@[j] matches Some(w) ==> self.heap.source_word(w)) by {
                if self.roots@[j] is Some {
                    let w = self.roots@[j]->0;
                    assert(c1.roots@[j] == self.roots@[j]);
                    assert(c1.heap.source_word(w));
                    assert(s0.root_spec(j) == self.roots@[j]);
                    assert(s0.root_bits()[j] == w.bits_spec());
                    assert(s0.root_bits().to_set().contains(w.bits_spec()));
                    assert(held.contains(w.bits_spec()));
                    if w.is_ptr_spec() {
                        assert(!r.contains(w.bits_spec()));
                        let b = w.bits_spec();
                        if c1.heap.core.dom().contains(b) {
                            assert(self.heap.core.state(b) == c1.heap.core.state(b));
                        }
                    }
                }
            }
        }
        // 積んでいる途中の解放があれば、ほかのスレッドがまだこのヒープのページに書いている
        if M::CHECKED && sink_pushing(self.heap.core.pages.sink()) {
            proof {
                self.lemma_root_spec();
            }
            return Err(HeapError::NotQuiescent);
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
                }
                Err(e)
            },
        }
    }
}

} // verus!
