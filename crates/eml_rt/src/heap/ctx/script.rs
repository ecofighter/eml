//! スクリプトの extern (docs/spec/object-model.md の「extern」)。引数、環境変数、子のプロセスを Host を通して
//! 扱う。何が読めて何が起きたかは約束せず (信頼の一覧の T11)、引数と結果の所有と RC の増減を証明する。

#[cfg(verus_only)]
use super::proof::*;
use super::*;
use crate::externs::{FALSE, Fault, TRUE};
use crate::host::{host_arg, host_arg_count, host_env_has, host_env_var};

verus! {

impl<M: Mode> Ctx<M> {
    /// `Env.arg_count`。スクリプトに渡した引数の数。
    pub fn env_arg_count(&mut self) -> (r: i64)
        ensures
            *final(self) == *old(self),
    {
        host_arg_count(&self.host) as i64
    }

    /// `Env.arg`。`Env.args ()` の `i` 番の引数の文字列を作り、呼ぶ側がその参照を持つ。範囲の外と UTF-8 でない引数は
    /// 何も作らずに誤りを返す。
    pub fn env_arg(&mut self, i: i64) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Err ==> *final(self) == *old(self),
            r matches Ok(w) ==> final(self).is_string(w),
            r matches Ok(w) ==> (old(self).held_ok() ==> {
                &&& final(self).held_ok()
                &&& final(self).holds(w)
                &&& final(self).evolved_by(*old(self), |x: usize| 0int, Set::empty())
            }),
    {
        match host_arg(&self.host, i) {
            Ok(text) => Ok(self.fresh_string(text.as_str())),
            Err(e) => Err(e),
        }
    }

    /// `text` の文字列を作り、呼ぶ側がその参照を持つ。Host から写した文言のほか、VM が `io_failed` の節に渡す文言を
    /// 作るのにも使う (docs/spec/vm.md の「extern の中断」)。
    pub fn fresh_string(&mut self, text: &str) -> (w: Word)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            final(self).is_string(w),
            old(self).held_ok() ==> {
                &&& final(self).held_ok()
                &&& final(self).holds(w)
                &&& final(self).evolved_by(*old(self), |x: usize| 0int, Set::empty())
            },
    {
        let ghost s0 = *self;
        let w = self.heap.alloc_string(text);
        if M::CHECKED {
            self.stats.string_bytes_copied = self.stats.string_bytes_copied.wrapping_add(
                text.len() as u64,
            );
        }
        proof {
            s0.heap.lemma_text_spec(w);
            self.heap.lemma_text_spec(w);
            if M::CHECKED {
                self.lemma_words_kept(s0);
            }
            if s0.held_ok() {
                self.lemma_made_ok(s0, w);
            }
        }
        w
    }

    /// `Env.has`。環境変数 `name` が設定されているかの `Bool` のタグを返す。`name` の参照を受け取って手放す。
    ///
    /// # Safety
    /// `copy_string` と同じ。
    pub unsafe fn env_has(&mut self, name: Word) -> (r: Result<i64, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(name),
            !M::CHECKED ==> old(self).is_string(name) && old(self).holds(name),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || (r matches Err(Fault::Heap(_))),
            (r matches Err(Fault::Heap(_))) ==> final(self).heap_spec().settled_heap() == old(
                self,
            ).heap_spec().settled_heap(),
            !M::CHECKED ==> r is Ok,
            r matches Ok(v) ==> v == TRUE || v == FALSE,
            old(self).held_ok() && old(self).is_string(name) && old(self).holds(name) ==> r is Ok
                || (r matches Err(Fault::Heap(e)) && chain_err(Err::<(), HeapError>(e))),
            old(self).held_ok() && old(self).is_string(name) && old(self).holds(name) && r is Ok
                ==> {
                &&& final(self).held_ok()
                &&& final(self).evolved_by(*old(self), |x: usize| -points(name, x), Set::empty())
            },
    {
        let ghost s0 = *self;
        let ghost c = s0.held_ok() && s0.is_string(name) && s0.holds(name);
        if M::CHECKED && !name.is_ptr() {
            return Err(Fault::Heap(HeapError::WrongKind));
        }
        self.heap.settle();
        let ghost h1 = self.heap;
        let hs = self.heap.header_of(name);
        proof {
            lemma_settled_twice(s0.heap);
            assert(h1.words_kept(s0.heap));
            h1.lemma_text_spec(name);
            s0.heap.lemma_text_spec(name);
            if c {
                s0.lemma_settled_body(h1, name, Kind::String);
            }
        }
        if let Err(e) = self.heap.checked_body(hs, Kind::String) {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        let set = host_env_has(&self.host, self.heap.string_text(name));
        self.heap.release_text(hs, Ghost(name), Ghost(c));
        proof {
            if M::CHECKED {
                lemma_words_trans(self.heap, h1, s0.heap);
                self.lemma_words_kept(s0);
            }
            if c {
                self.lemma_printed(s0, h1, name);
            }
        }
        Ok(
            if set {
                TRUE
            } else {
                FALSE
            },
        )
    }

    /// `Env.lookup`。環境変数 `name` の値の文字列を作り、呼ぶ側がその参照を持つ。設定されていなければ空の文字列である。
    /// `name` の参照を受け取り、値を読めたかどうかによらず手放す。
    ///
    /// # Safety
    /// `copy_string` と同じ。
    pub unsafe fn env_var(&mut self, name: Word) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(name),
            !M::CHECKED ==> old(self).is_string(name) && old(self).holds(name),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            (r matches Err(Fault::Heap(_))) ==> final(self).heap_spec().settled_heap() == old(
                self,
            ).heap_spec().settled_heap(),
            !M::CHECKED ==> !(r matches Err(Fault::Heap(_))),
            r matches Ok(w) ==> final(self).is_string(w),
            old(self).held_ok() && old(self).is_string(name) && old(self).holds(name) ==> !(
            r matches Err(Fault::Heap(_))) || (r matches Err(Fault::Heap(e)) && chain_err(
                Err::<(), HeapError>(e),
            )),
            old(self).held_ok() && old(self).is_string(name) && old(self).holds(name) && !(
            r matches Err(Fault::Heap(_))) ==> {
                &&& final(self).held_ok()
                &&& final(self).evolved_by(*old(self), |x: usize| -points(name, x), Set::empty())
                &&& r matches Ok(w) ==> final(self).holds(w)
            },
    {
        let ghost s0 = *self;
        let ghost c = s0.held_ok() && s0.is_string(name) && s0.holds(name);
        if M::CHECKED && !name.is_ptr() {
            return Err(Fault::Heap(HeapError::WrongKind));
        }
        self.heap.settle();
        let ghost h1 = self.heap;
        let hs = self.heap.header_of(name);
        proof {
            lemma_settled_twice(s0.heap);
            assert(h1.words_kept(s0.heap));
            h1.lemma_text_spec(name);
            s0.heap.lemma_text_spec(name);
            if c {
                s0.lemma_settled_body(h1, name, Kind::String);
            }
        }
        if let Err(e) = self.heap.checked_body(hs, Kind::String) {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        match host_env_var(&self.host, self.heap.string_text(name)) {
            Ok(value) => {
                // 値を作ってから名前を手放す。先に手放すと、空けた名前の箱を値が使い回しうるので、呼ぶ前からの変化
                // (`evolved_by`) を証明で言えない (`fs_open` と同じ)
                let value = value.as_str();
                let w = self.heap.alloc_string(value);
                let ghost h2 = self.heap;
                if M::CHECKED {
                    self.stats.string_bytes_copied = self.stats.string_bytes_copied.wrapping_add(
                        value.len() as u64,
                    );
                }
                proof {
                    h2.lemma_text_spec(w);
                    self.lemma_room_made(h1, name, w, Kind::String);
                    if c {
                        s0.lemma_path_kept(h1, h2, name, w);
                    }
                }
                self.heap.release_text(hs, Ghost(name), Ghost(c));
                proof {
                    self.heap.lemma_text_spec(w);
                    if M::CHECKED {
                        lemma_words_trans(h2, h1, s0.heap);
                        lemma_words_trans(self.heap, h2, s0.heap);
                        self.lemma_words_kept(s0);
                    }
                    if c {
                        self.lemma_opened(s0, h1, h2, name, w);
                    }
                }
                Ok(w)
            },
            Err(e) => {
                self.heap.release_text(hs, Ghost(name), Ghost(c));
                proof {
                    if M::CHECKED {
                        lemma_words_trans(self.heap, h1, s0.heap);
                        self.lemma_words_kept(s0);
                    }
                    if c {
                        self.lemma_printed(s0, h1, name);
                    }
                }
                Err(e)
            },
        }
    }
}

} // verus!
