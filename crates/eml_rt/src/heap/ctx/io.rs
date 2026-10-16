//! 出力とファイルの extern (docs/spec/object-model.md の「extern」)。Host を通して書き、開き、読む。何が書かれ、
//! 何が読めたかは約束せず (信頼の一覧の T11)、引数と結果の所有と RC の増減を証明する。

#[cfg(verus_only)]
use super::proof::*;
use super::*;
use crate::externs::{Fault, IoOp, TUPLE, Utf8Source};
use crate::host::{File, ReadError, Stream, host_flush, host_open, host_write_line, read_text};
#[cfg(verus_only)]
use vstd::string::StringSliceAdditionalSpecFns;

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

    /// 文字列 `s` の中身を、先頭から `max_chars` 文字まで Rust の `String` に写して返す。`s` の参照を受け取って手放す。
    /// REPL が式の入力の結果を整形するのに使う。写す長さを切るのは、利用者の `show` が巨大な文字列を返しても、確保量の
    /// 上限の外で同じ大きさを写さないためである (docs/spec/object-model.md の「実行文脈 `Ctx`」)。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。`s` は即値か、このヒープか付けた塊の生きている物体を指し、呼ぶ側がその参照を1つ
    /// 持つ。速いモードでは、`s` は文字列である (検査モードは確かめ、違えば何も写さずに `WrongKind` を返す)。
    pub unsafe fn copy_string(&mut self, s: Word, max_chars: usize) -> (r: Result<String, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(s),
            !M::CHECKED ==> old(self).is_string(s) && old(self).holds(s),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || (r matches Err(Fault::Heap(_))),
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
        if let Err(e) = self.heap.checked_string(hs) {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        let copied = prefix_chars(self.heap.string_text(s), max_chars).to_owned();
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
        Ok(copied)
    }

    /// `line` と `\n` を標準出力に書き、書き出す。REPL が整形した結果を書くのに使う。Host の境界 (T11) は何も約束しない
    /// ので、書けたかどうかだけを返す。
    pub fn print_line(&mut self, line: &str) -> (r: Result<(), Fault>)
        ensures
            *final(self) == *old(self),
            r is Ok || (r matches Err(Fault::Output(_))),
    {
        if let Err(e) = host_write_line(&self.host, Stream::Out, line) {
            return Err(Fault::Output(e));
        }
        match host_flush(&self.host) {
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

    /// `Fs.open`。パス `path` のファイルを Host に開かせ、`lin` ビットを立てた `File` の物体を返す。`path` の参照を
    /// 受け取り、開けたかどうかによらず手放す。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。`path` は即値か、このヒープか付けた塊の生きている物体を指し、呼ぶ側がその参照を
    /// 1つ持つ。速いモードでは、`path` は文字列である (検査モードは確かめ、違えば何も開かずに `WrongKind` を返す)。
    pub unsafe fn fs_open(&mut self, path: Word) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(path),
            !M::CHECKED ==> old(self).is_string(path) && old(self).holds(path),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || (r matches Err(Fault::Io { op: IoOp::Open, .. })) || (r matches Err(
                Fault::Heap(_),
            )),
            (r matches Err(Fault::Heap(_))) ==> final(self).heap_spec().settled_heap() == old(
                self,
            ).heap_spec().settled_heap(),
            !M::CHECKED ==> !(r matches Err(Fault::Heap(_))),
            r matches Ok(f) ==> final(self).heap_spec().kind_of(f) == Kind::Foreign
                && final(self).heap_spec().lin_of(f),
            old(self).held_ok() && old(self).is_string(path) && old(self).holds(path) ==> !(
            r matches Err(Fault::Heap(_))) || (r matches Err(Fault::Heap(e)) && chain_err(
                Err::<(), HeapError>(e),
            )),
            old(self).held_ok() && old(self).is_string(path) && old(self).holds(path) && !(
            r matches Err(Fault::Heap(_))) ==> {
                &&& final(self).held_ok()
                &&& final(self).evolved_by(*old(self), |x: usize| -points(path, x), Set::empty())
                &&& r matches Ok(f) ==> final(self).holds(f)
            },
    {
        let ghost s0 = *self;
        let ghost c = s0.held_ok() && s0.is_string(path) && s0.holds(path);
        if M::CHECKED && !path.is_ptr() {
            return Err(Fault::Heap(HeapError::WrongKind));
        }
        self.heap.settle();
        let ghost h1 = self.heap;
        let hp = self.heap.header_of(path);
        proof {
            lemma_settled_twice(s0.heap);
            assert(h1.words_kept(s0.heap));
            h1.lemma_text_spec(path);
            s0.heap.lemma_text_spec(path);
            if c {
                s0.lemma_settled_string(h1, path);
            }
        }
        // 確かめはどれも Host を呼ぶ前に済ませ、誤りなら何も開かない
        if let Err(e) = self.heap.checked_string(hp) {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        match host_open(&self.host, self.heap.string_text(path)) {
            Ok(file) => {
                // パスを手放す前に File を作る。先に手放すと、空けたパスの箱を File が使い回しうるので、呼ぶ前からの変化
                // (`evolved_by`) を証明で言えない
                let f = self.heap.alloc_foreign(file, true);
                let ghost h2 = self.heap;
                proof {
                    self.lemma_room_made(h1, path, f);
                    if c {
                        s0.lemma_path_kept(h1, h2, path, f);
                    }
                }
                self.heap.release_text(hp, Ghost(path), Ghost(c));
                proof {
                    if M::CHECKED {
                        lemma_words_trans(h2, h1, s0.heap);
                        lemma_words_trans(self.heap, h2, s0.heap);
                        self.lemma_words_kept(s0);
                    }
                    if c {
                        self.lemma_opened(s0, h1, h2, path, f);
                    }
                }
                Ok(f)
            },
            Err((p, reason)) => {
                self.heap.release_text(hp, Ghost(path), Ghost(c));
                proof {
                    if M::CHECKED {
                        lemma_words_trans(self.heap, h1, s0.heap);
                        self.lemma_words_kept(s0);
                    }
                    if c {
                        self.lemma_printed(s0, h1, path);
                    }
                }
                Err(Fault::Io { op: IoOp::Open, name: p, reason })
            },
        }
    }

    /// `Fs.read_all`。`file` を終わりまで読み、組 `(File, String)` を返す。`file` の参照は組に移る。64 KiB ずつ読み、
    /// 区切りごとに止める語と確保量を見る (docs/spec/object-model.md の「実行の制御」)。読みの誤り、UTF-8 でない中身、
    /// 止めたときは、`file` を手放して閉じ、結果を作らない。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。`file` は即値か、このヒープか付けた塊の生きている物体を指し、呼ぶ側がその参照を
    /// 1つ持つ。速いモードでは、`file` は局所で rc が 1 である (検査モードは確かめ、違えば `NotUnique` か `WrongOwner`
    /// を返す)。`File` でなければ、どちらのモードでも `WrongKind` を返す。
    pub unsafe fn fs_read_all(&mut self, file: Word) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(file),
            !M::CHECKED ==> old(self).holds(file) && old(self).heap_spec().rc_value(file) == 1,
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || (r matches Err(Fault::Io { op: IoOp::Read, .. })) || (r matches Err(
                Fault::NotUtf8(_),
            )) || (r matches Err(Fault::Stopped(_))) || (r matches Err(Fault::Heap(_))),
            (r matches Err(Fault::Heap(_))) ==> *final(self) == *old(self)
                || final(self).heap_spec().kept_live(old(self).heap_spec()),
            !M::CHECKED && (r matches Err(Fault::Heap(_))) ==> r == Err::<Word, Fault>(
                Fault::Heap(HeapError::WrongKind),
            ) && *final(self) == *old(self),
            r matches Ok(t) ==> {
                let h = final(self).heap_spec();
                &&& h.kind_of(t) == Kind::Data
                &&& h.tag_of(t) == TUPLE
                &&& h.lin_of(t)
                &&& h.count_of(t) == 2
                &&& h.scanned_word(t, 0) == file
                &&& final(self).is_string(h.scanned_word(t, 1))
            },
            old(self).held_ok() && old(self).holds(file) && old(self).heap_spec().rc_value(file)
                == 1 ==> (r matches Err(Fault::Heap(e)) ==> *final(self) == *old(self) || chain_err(
                Err::<(), HeapError>(e),
            )),
            old(self).held_ok() && old(self).holds(file) && old(self).heap_spec().rc_value(file)
                == 1 && !(r matches Err(Fault::Heap(_))) ==> {
                &&& final(self).held_ok()
                &&& final(self).evolved_by(*old(self), |x: usize| -points(file, x), Set::empty())
                &&& r matches Ok(t) ==> final(self).holds(t)
            },
    {
        let ghost s0 = *self;
        let ghost c = s0.held_ok() && s0.holds(file) && s0.heap.rc_value(file) == 1;
        if !file.is_ptr() {
            return Err(Fault::Heap(HeapError::WrongKind));
        }
        proof {
            if c {
                s0.lemma_held_arg(file);
            }
        }
        let p = match unsafe { self.heap.foreign_unique::<File>(file) } {
            Ok(p) => p,
            Err(e) => return Err(Fault::Heap(e)),
        };
        let text = match unsafe { read_text(p, &self.control) } {
            Ok(text) => text,
            Err(e) => {
                // 手放すと破棄処理が File を閉じる
                if let Err(e) = unsafe { self.heap.decref(file) } {
                    proof {
                        self.lemma_words_kept(s0);
                    }
                    return Err(Fault::Heap(e));
                }
                proof {
                    if M::CHECKED {
                        self.lemma_words_kept(s0);
                    }
                    if c {
                        self.lemma_closed(s0, file);
                    }
                }
                return Err(
                    match e {
                        ReadError::Io(path, reason) => Fault::Io {
                            op: IoOp::Read,
                            name: path,
                            reason,
                        },
                        ReadError::NotUtf8(path) => Fault::NotUtf8(
                            Box::new(Utf8Source::File(path)),
                        ),
                        ReadError::Stopped(stop) => Fault::Stopped(stop),
                    },
                );
            },
        };
        let text = text.as_str();
        let s = self.heap.alloc_string(text);
        let ghost h1 = self.heap;
        if M::CHECKED {
            self.stats.string_bytes_copied = self.stats.string_bytes_copied.wrapping_add(
                text.len() as u64,
            );
        }
        let t = self.heap.alloc_data(TUPLE, 2, 0, true);
        let ghost h2 = self.heap;
        proof {
            lemma_words_trans(h2, h1, s0.heap);
            lemma_unextended(h2, t);
            assert(h2.scanned_at(t, 0) && h2.scanned_at(t, 1));
            if c {
                s0.lemma_read_mid(h1, h2, s, t, file);
            }
        }
        // `file` の参照は組に移るので、`dup` しない
        (unsafe { self.heap.set_field(t, 0, file) })?;
        let ghost h3 = self.heap;
        proof {
            if M::CHECKED {
                lemma_set_passes_kept(h3, h2, t, 0, file, 1);
            }
            if c {
                s0.lemma_read_set(h1, h2, h3, s, t, file);
            }
        }
        (unsafe { self.heap.set_field(t, 1, s) })?;
        proof {
            lemma_words_trans(h3, h2, s0.heap);
            lemma_words_trans(self.heap, h3, s0.heap);
            self.lemma_words_kept(s0);
            self.lemma_tuple_made(s0.heap, h1, h2, h3, s, t, file);
            if c {
                self.lemma_read_ok(s0, h1, h2, h3, s, t, file);
            }
        }
        Ok(t)
    }

    /// `Fs.close`。`file` の参照を手放す。最後の参照なので、破棄処理が File を閉じる。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。`file` は即値か、このヒープか付けた塊の生きている物体を指し、呼ぶ側がその参照を
    /// 1つ持つ。`File` でなければ、どちらのモードでも `WrongKind` を返す。
    pub unsafe fn fs_close(&mut self, file: Word) -> (r: Result<(), Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(file),
            !M::CHECKED ==> old(self).holds(file),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || (r matches Err(Fault::Heap(_))),
            r is Err ==> *final(self) == *old(self) || final(self).heap_spec().kept_live(
                old(self).heap_spec(),
            ),
            !M::CHECKED && r is Err ==> r == Err::<(), Fault>(Fault::Heap(HeapError::WrongKind))
                && *final(self) == *old(self),
            old(self).held_ok() && old(self).holds(file) ==> (r matches Err(Fault::Heap(e))
                ==> *final(self) == *old(self) || chain_err(Err::<(), HeapError>(e))),
            old(self).held_ok() && old(self).holds(file) && r is Ok ==> {
                &&& final(self).held_ok()
                &&& final(self).evolved_by(*old(self), |x: usize| -points(file, x), Set::empty())
            },
    {
        let ghost s0 = *self;
        let ghost c = s0.held_ok() && s0.holds(file);
        if !file.is_ptr() {
            return Err(Fault::Heap(HeapError::WrongKind));
        }
        proof {
            if c {
                s0.lemma_held_arg(file);
            }
        }
        if let Err(e) = unsafe { self.heap.foreign::<File>(file) } {
            return Err(Fault::Heap(e));
        }
        if M::CHECKED {
            let h = self.heap.header_of(file);
            if let Err(e) = self.heap.checked_decref(h) {
                return Err(Fault::Heap(e));
            }
        }
        if let Err(e) = unsafe { self.heap.decref(file) } {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        proof {
            if M::CHECKED {
                self.lemma_words_kept(s0);
            }
            if c {
                self.lemma_closed(s0, file);
            }
        }
        Ok(())
    }

    /// 呼ぶ側が持つ引数 `w` は、読めて、`decref` できる。
    proof fn lemma_held_arg(&self, w: Word)
        requires
            self.held_ok(),
            self.holds(w),
        ensures
            self.heap.consistent(),
            self.heap.settleable(),
            self.heap.readable(w),
            self.heap.owned(w),
            self.heap.decref_ok(w),
    {
        let ws = seq![w];
        self.lemma_arg_counted(ws, 0);
        lemma_points_in(ws, w.bits_spec());
        lemma_moved_refl(self.heap);
        lemma_arg_moved(self.heap, self.heap, |x: usize| 0int, Set::empty(), w);
    }

    /// 呼ぶ側の持つ `w` を手放しただけの変化の後も、`Ctx` の語は持たれている。
    proof fn lemma_closed(&self, s0: Ctx<M>, w: Word)
        requires
            s0.held_ok(),
            s0.holds(w),
            self.heap.consistent(),
            self.heap.settled(),
            self.heap.evolved(s0.heap, w, -1),
            self.heap.kept_live(s0.heap),
            self.evv == s0.evv,
            self.yielding == s0.yielding,
            self.max_evv == s0.max_evv,
        ensures
            self.held_ok(),
            self.evolved_by(s0, |x: usize| -points(w, x), Set::empty()),
    {
        let dw = |x: usize| -points(w, x);
        let none = Set::<usize>::empty();
        let ws = seq![w];
        lemma_decref_moved(self.heap, s0.heap, w);
        assert forall|x: usize| #[trigger] self.need(seq![], x) > 0 implies s0.need(ws, x) + dw(x)
            >= self.need(seq![], x) by {
            lemma_points_in(seq![], x);
            lemma_points_in(ws, x);
        }
        self.lemma_holds_moved(s0, ws, seq![], dw, none);
        self.lemma_evv_kept(s0, dw, none);
    }

    /// `Fs.open` で File を作った後も、パス `s` は持たれていて手放せる。`self` は呼ぶ前の文脈である。
    proof fn lemma_path_kept(&self, h1: Heap<M>, h2: Heap<M>, s: Word, w: Word)
        requires
            self.held_ok(),
            self.is_string(s),
            self.holds(s),
            h1 == self.heap.settled_heap(),
            h1.consistent(),
            h2.made(h1, w),
        ensures
            h2.consistent(),
            h2.core.state(s.bits_spec()) == BlockState::Live,
            h2.core.rc_of(s.bits_spec()) != 0 ==> h2.core.ext(s.bits_spec()) >= 1,
    {
        self.lemma_settled_string(h1, s);
        lemma_alloc_moved(h2, h1, w);
        lemma_arg_moved(h2, h1, |x: usize| 0int, Set::empty(), s);
        lemma_view_live(h2, s.bits_spec());
    }

    /// `Fs.open` の、開けた道でモデルが合っている道。`h1` は `settle` の後、`h2` は File を作った後である。
    proof fn lemma_opened(&self, s0: Ctx<M>, h1: Heap<M>, h2: Heap<M>, s: Word, w: Word)
        requires
            s0.held_ok(),
            s0.is_string(s),
            s0.holds(s),
            h1 == s0.heap.settled_heap(),
            h1.consistent(),
            h2.made(h1, w),
            h2.owned(w),
            h2.consistent(),
            self.heap.consistent(),
            self.heap.core.model@.building is None,
            self.heap.evolved(h2, s, -1),
            self.heap.kept_live(h2),
            self.evv == s0.evv,
            self.yielding == s0.yielding,
            self.max_evv == s0.max_evv,
        ensures
            self.held_ok(),
            self.holds(w),
            self.evolved_by(s0, |x: usize| -points(s, x), Set::empty()),
    {
        let h0 = s0.heap;
        let z = |x: usize| 0int;
        let ds = |x: usize| -points(s, x);
        let none = Set::<usize>::empty();
        let ws = seq![s];
        s0.lemma_settled_string(h1, s);
        lemma_settle_moved(h1, h0);
        lemma_alloc_moved(h2, h1, w);
        lemma_decref_moved(self.heap, h2, s);
        lemma_moved_chain(self.heap, h2, h1, z, ds, ds, none, none, none);
        lemma_moved_chain(self.heap, h1, h0, z, ds, ds, none, none, none);
        assert forall|x: usize| #[trigger] self.need(seq![], x) > 0 implies s0.need(ws, x) + ds(x)
            >= self.need(seq![], x) by {
            lemma_points_in(seq![], x);
            lemma_points_in(ws, x);
        }
        self.lemma_holds_moved(s0, ws, seq![], ds, none);
        self.lemma_evv_kept(s0, ds, none);
        assert(s.bits_spec() != w.bits_spec());
        lemma_arg_moved(self.heap, h2, ds, none, w);
        lemma_view_live(self.heap, w.bits_spec());
        self.lemma_fresh_holds(s0, w);
    }

    /// `Fs.read_all` で文字列 `s` と組 `t` を作った後の、モデルが合っている道。`self` は呼ぶ前の文脈で、組の 0 番に
    /// `file` を書ける。
    proof fn lemma_read_mid(&self, h1: Heap<M>, h2: Heap<M>, s: Word, t: Word, file: Word)
        requires
            self.held_ok(),
            self.holds(file),
            h1.made(self.heap, s),
            h2.made(h1, t),
        ensures
            h2.consistent(),
            moved(h2, self.heap, |x: usize| 0int, Set::empty()),
            !live(self.heap, s.bits_spec()),
            live(h2, t.bits_spec()),
            file.bits_spec() != s.bits_spec() && file.bits_spec() != t.bits_spec(),
            h2.core.settled_view().ext(s.bits_spec()) == 1,
            held_word(h2, file),
            h2.holds(file),
            h2.writable(t),
    {
        let h0 = self.heap;
        let z = |x: usize| 0int;
        let none = Set::<usize>::empty();
        let ws = seq![file];
        self.lemma_arg_counted(ws, 0);
        lemma_points_in(ws, file.bits_spec());
        lemma_alloc_moved(h1, h0, s);
        lemma_alloc_moved(h2, h1, t);
        lemma_moved_chain(h2, h1, h0, z, z, z, none, none, none);
        lemma_arg_moved(h2, h0, z, none, file);
        assert(live(h2, s.bits_spec()));
        assert(exact(h2, h1, z));
        assert(live(h1, s.bits_spec()));
    }

    /// 組の 0 番に `file` を書いた後も、1 番に `s` を書ける。`self` は呼ぶ前の文脈である。
    proof fn lemma_read_set(
        &self,
        h1: Heap<M>,
        h2: Heap<M>,
        h3: Heap<M>,
        s: Word,
        t: Word,
        file: Word,
    )
        requires
            self.held_ok(),
            self.holds(file),
            h1.made(self.heap, s),
            h1.owned(s),
            h2.made(h1, t),
            h2.owned(t),
            h2.scan_of(t) == 2,
            forall|i: int| 0 <= i < 2 ==> (#[trigger] h2.scanned_word(t, i)).bits_spec() == 1,
            h3.field_set(h2, t, 0, file),
            h3.consistent(),
        ensures
            h3.writable(t),
            h3.holds(s),
            h3.owned(t),
            h3.scanned_at(t, 1),
            h3.word_at(t, 1).bits_spec() == 1,
            held_word(h3, s),
    {
        let df = |x: usize| -points(file, x);
        let x = Set::<usize>::empty().insert(t.bits_spec());
        self.lemma_read_mid(h1, h2, s, t, file);
        lemma_unextended(h2, t);
        assert(h2.scanned_word(t, 0).bits_spec() == 1);
        assert(h2.scanned_word(t, 1).bits_spec() == 1);
        lemma_set_unit_moved(h3, h2, t, 0, file);
        lemma_unextended(h3, t);
        assert(h3.word_at(t, 1) == h2.word_at(t, 1));
        lemma_view_live(h2, s.bits_spec());
        assert(held_word(h2, s));
        lemma_arg_moved(h3, h2, df, x, s);
        assert(live(h3, t.bits_spec()));
        assert(same_head(h3, h2, t.bits_spec()));
    }

    /// `Fs.read_all` が返す組の形。モデルが合わなくても言える。
    proof fn lemma_tuple_made(
        &self,
        h0: Heap<M>,
        h1: Heap<M>,
        h2: Heap<M>,
        h3: Heap<M>,
        s: Word,
        t: Word,
        file: Word,
    )
        requires
            h1.made(h0, s),
            h1.is_string(s),
            h2.grew(h1, t),
            h2.kind_of(t) == Kind::Data,
            h2.tag_of(t) == TUPLE,
            h2.lin_of(t),
            h2.count_of(t) == 2,
            h2.scan_of(t) == 2,
            h3.field_set(h2, t, 0, file),
            self.heap.field_set(h3, t, 1, s),
        ensures
            self.heap.kind_of(t) == Kind::Data,
            self.heap.tag_of(t) == TUPLE,
            self.heap.lin_of(t),
            self.heap.count_of(t) == 2,
            self.heap.scanned_word(t, 0) == file,
            self.heap.scanned_word(t, 1) == s,
            self.is_string(s),
    {
        assert(h2.grown_block(h1, s.bits_spec()));
        lemma_unextended(h2, t);
        lemma_unextended(h3, t);
        lemma_unextended(self.heap, t);
        self.heap.lemma_text_spec(s);
        h1.lemma_text_spec(s);
        h2.lemma_text_spec(s);
        h3.lemma_text_spec(s);
    }

    /// `Fs.read_all` の、読めた道でモデルが合っている道。
    proof fn lemma_read_ok(
        &self,
        s0: Ctx<M>,
        h1: Heap<M>,
        h2: Heap<M>,
        h3: Heap<M>,
        s: Word,
        t: Word,
        file: Word,
    )
        requires
            s0.held_ok(),
            s0.holds(file),
            self.evv == s0.evv,
            self.yielding == s0.yielding,
            self.max_evv == s0.max_evv,
            h1.made(s0.heap, s),
            h1.owned(s),
            h2.made(h1, t),
            h2.owned(t),
            h2.scan_of(t) == 2,
            forall|i: int| 0 <= i < 2 ==> (#[trigger] h2.scanned_word(t, i)).bits_spec() == 1,
            h3.field_set(h2, t, 0, file),
            self.heap.field_set(h3, t, 1, s),
            h3.consistent(),
            self.heap.consistent(),
        ensures
            self.held_ok(),
            self.holds(t),
            self.evolved_by(s0, |x: usize| -points(file, x), Set::empty()),
    {
        let h0 = s0.heap;
        let h4 = self.heap;
        let z = |x: usize| 0int;
        let none = Set::<usize>::empty();
        let x = none.insert(t.bits_spec());
        let df = |b: usize| -points(file, b);
        let ds = |b: usize| -points(s, b);
        let d = |b: usize| -points(file, b) - points(s, b);
        s0.lemma_read_mid(h1, h2, s, t, file);
        s0.lemma_read_set(h1, h2, h3, s, t, file);
        let tb = t.bits_spec();
        let sb = s.bits_spec();
        lemma_unextended(h2, t);
        assert(h2.scanned_word(t, 0).bits_spec() == 1);
        lemma_set_unit_moved(h3, h2, t, 0, file);
        lemma_set_unit_moved(h4, h3, t, 1, s);
        // 呼ぶ前から生きている物体についての変化
        lemma_moved_chain(h3, h2, h0, z, df, df, none, x, none);
        assert forall|b: usize| live(h0, b) implies #[trigger] df(b) == df(b) + ds(b) by {}
        lemma_moved_chain(h4, h3, h0, df, ds, df, none, x, none);
        // 新しい組と文字列についての変化
        lemma_moved_chain(h4, h3, h2, df, ds, d, x, x, x);
        assert(live(h2, tb));
        assert(d(tb) == 0);
        assert(live(h4, tb));
        assert(same_head(h4, h2, tb));
        // Ctx の語と引数
        let ws = seq![file];
        assert forall|b: usize| #[trigger] self.need(seq![], b) > 0 implies s0.need(ws, b) + df(b)
            >= self.need(seq![], b) by {
            lemma_points_in(seq![], b);
            lemma_points_in(ws, b);
        }
        self.lemma_holds_moved(s0, ws, seq![], df, none);
        self.lemma_evv_kept(s0, df, none);
        lemma_view_live(h4, tb);
        assert(held_word(h4, t));
        self.lemma_fresh_holds(s0, t);
    }

    /// `println` の、モデルが合っている道。`settle` の後の `h1` から `s` を手放した。
    pub(super) proof fn lemma_printed(&self, s0: Ctx<M>, h1: Heap<M>, s: Word)
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

/// `text` の先頭の `max_chars` 文字。文字の始まりを `is_char_boundary` で数えるので、多バイトの文字の途中で切らない。
fn prefix_chars(text: &str, max_chars: usize) -> (r: &str) {
    let len = text.len();
    let mut end: usize = 0;
    let mut count: usize = 0;
    // `max_chars` 文字を数えた後の、次の文字の始まりで止まる
    while end < len && (count < max_chars || !text.is_char_boundary(end))
        invariant
            end <= len,
            count <= max_chars,
        decreases len - end,
    {
        if text.is_char_boundary(end) {
            count += 1;
        }
        end += 1;
    }
    if text.is_char_boundary(end) {
        text.split_at(end).0
    } else {
        text
    }
}

} // verus!
