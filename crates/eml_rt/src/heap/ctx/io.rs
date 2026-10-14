//! 出力の extern。Host を通して書く。何が書かれたかは約束せず (信頼の一覧の T11)、文字列の所有と RC の増減を証明する。

#[cfg(verus_only)]
use super::proof::*;
use super::*;
use crate::externs::Fault;
use crate::host::{Stream, host_flush, host_write_line};

verus! {

impl<M: Mode> Ctx<M> {
    /// `println`。文字列 `s` と `\n` を標準出力に書く。`s` の参照を受け取り、書けたかどうかによらず手放す。Host が出力を
    /// まとめて書くときは、`Err(Output)` は、この行かそれより前の行を書けなかったことを表す。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。`s` は即値か、このヒープか付けた塊の生きている物体を指し、呼ぶ側がその参照を1つ
    /// 持つ。速いモードでは、`s` は文字列である (検査モードは確かめ、違えば何も書かずに `WrongKind` を返す)。
    pub unsafe fn println(&mut self, s: Word) -> (r: Result<(), Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(s),
            !M::CHECKED ==> old(self).is_string(s) && old(self).holds(s),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || (r matches Err(Fault::Output(_))) || (r matches Err(Fault::Heap(_))),
            (r matches Err(Fault::Heap(_))) ==> final(self).heap_spec().settled_heap() == old(
                self,
            ).heap_spec().settled_heap(),
            !M::CHECKED ==> !(r matches Err(Fault::Heap(_))),
            old(self).held_ok() && old(self).is_string(s) && old(self).holds(s) ==> !(r matches Err(
                Fault::Heap(_),
            )) || (r matches Err(Fault::Heap(e)) && chain_err(Err::<(), HeapError>(e))),
            old(self).held_ok() && old(self).is_string(s) && old(self).holds(s) && !(r matches Err(
                Fault::Heap(_),
            )) ==> {
                &&& final(self).held_ok()
                &&& final(self).evolved_by(*old(self), |x: usize| -points(s, x), Set::empty())
            },
    {
        let ghost s0 = *self;
        let ghost c = s0.held_ok() && s0.is_string(s) && s0.holds(s);
        if M::CHECKED && !s.is_ptr() {
            return Err(Fault::Heap(HeapError::WrongKind));
        }
        self.heap.settle();
        let ghost h1 = self.heap;
        let hs = self.heap.header_of(s);
        proof {
            lemma_settled_twice(s0.heap);
            assert(h1.words_kept(s0.heap));
            h1.lemma_text_spec(s);
            s0.heap.lemma_text_spec(s);
            if c {
                s0.lemma_settled_string(h1, s);
            }
        }
        // 確かめはどれも Host を呼ぶ前に済ませ、誤りなら何も書かない
        if let Err(e) = self.heap.checked_string(hs) {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        let written = host_write_line(&self.host, Stream::Out, self.heap.string_text(s));
        self.heap.release_text(hs, Ghost(s), Ghost(c));
        proof {
            if M::CHECKED {
                lemma_words_trans(self.heap, h1, s0.heap);
                self.lemma_words_kept(s0);
            }
            if c {
                self.lemma_printed(s0, h1, s);
            }
        }
        match written {
            Ok(()) => Ok(()),
            Err(e) => Err(Fault::Output(e)),
        }
    }

    /// Host がまとめて持っている出力を書かせる。VM は、プログラムの終わり、実行時エラーを出す前、止めたときに呼ぶ。
    pub fn flush(&mut self) -> (r: Result<(), Fault>)
        ensures
            *final(self) == *old(self),
            r is Ok || (r matches Err(Fault::Output(_))),
    {
        match host_flush(&self.host) {
            Ok(()) => Ok(()),
            Err(e) => Err(Fault::Output(e)),
        }
    }

    /// `println` の、モデルが合っている道。`settle` の後の `h1` から `s` を手放した。
    proof fn lemma_printed(&self, s0: Ctx<M>, h1: Heap<M>, s: Word)
        requires
            s0.held_ok(),
            s0.is_string(s),
            s0.holds(s),
            h1 == s0.heap.settled_heap(),
            h1.consistent(),
            self.heap.consistent(),
            self.heap.core.model@.building is None,
            self.heap.evolved(h1, s, -1),
            self.heap.kept_live(h1),
            self.evv == s0.evv,
            self.yielding == s0.yielding,
            self.max_evv == s0.max_evv,
        ensures
            self.held_ok(),
            self.evolved_by(s0, |x: usize| -points(s, x), Set::empty()),
    {
        let h0 = s0.heap;
        let z = |x: usize| 0int;
        let ds = |x: usize| -points(s, x);
        let none = Set::<usize>::empty();
        let ws = seq![s];
        lemma_settle_moved(h1, h0);
        lemma_decref_moved(self.heap, h1, s);
        lemma_moved_chain(self.heap, h1, h0, z, ds, ds, none, none, none);
        assert forall|x: usize| #[trigger] self.need(seq![], x) > 0 implies s0.need(ws, x) + ds(x)
            >= self.need(seq![], x) by {
            lemma_points_in(seq![], x);
            lemma_points_in(ws, x);
        }
        self.lemma_holds_moved(s0, ws, seq![], ds, none);
        self.lemma_evv_kept(s0, ds, none);
    }
}

} // verus!
