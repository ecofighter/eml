//! スクリプトの extern (docs/spec/object-model.md の「extern」)。引数、環境変数、標準入力、ファイルの書き込み、子の
//! プロセスを Host を通して扱う。何が読めて何が起きたかは約束せず (信頼の一覧の T11)、引数と結果の所有と RC の増減を
//! 証明する。

#[cfg(verus_only)]
use super::proof::*;
use super::*;
use crate::externs::{FALSE, Fault, TRUE};
use crate::host::{
    host_arg, host_arg_count, host_env_has, host_env_var, host_read_stdin, host_run,
    host_write_file,
};

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

    /// `Stdin.read_all` と `Stdin.read_line_raw`。標準出力を書き出してから標準入力を読み、読んだ文字列を作る。`line` なら
    /// 次の `\n` まで (それを含めて) 読み、終わりなら空の文字列である。
    pub fn stdin_read(&mut self, line: bool) -> (r: Result<Word, Fault>)
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
        match host_read_stdin(&self.host, &self.control, line) {
            Ok(text) => Ok(self.fresh_string(text.as_str())),
            Err(e) => Err(e),
        }
    }

    /// `text` の文字列を作り、呼ぶ側がその参照を持つ。Host から写した文言のほか、VM が `io_failed` の節に渡す文言を
    /// 作るのにも使う (docs/spec/vm.md の「IO の失敗と `exit`」)。
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
                s0.lemma_settled_string(h1, name);
            }
        }
        if let Err(e) = self.heap.checked_string(hs) {
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

    /// `Env.var`。環境変数 `name` の値の文字列を作り、呼ぶ側がその参照を持つ。設定されていなければ空の文字列である。
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
                s0.lemma_settled_string(h1, name);
            }
        }
        if let Err(e) = self.heap.checked_string(hs) {
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
                    self.lemma_room_made(h1, name, w);
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

    /// `Fs.write_text`。ファイル `path` を作るか切り詰めて、`text` を書く。読み終えたら `path`、`text` の順に手放す。
    ///
    /// # Safety
    /// `string_eq` と同じ (`a` が `path`、`b` が `text`)。
    pub unsafe fn fs_write_text(&mut self, path: Word, text: Word) -> (r: Result<(), Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(path),
            old(self).heap_spec().word_in(text),
            !M::CHECKED ==> old(self).holds_strings(path, text),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            (r matches Err(Fault::Heap(_))) ==> final(self).heap_spec().settled_heap() == old(
                self,
            ).heap_spec().settled_heap(),
            !M::CHECKED ==> !(r matches Err(Fault::Heap(_))),
            old(self).held_ok() && old(self).holds_strings(path, text) ==> !(r matches Err(
                Fault::Heap(_),
            )) || (r matches Err(Fault::Heap(e)) && chain_err(Err::<(), HeapError>(e))),
            old(self).held_ok() && old(self).holds_strings(path, text) && !(r matches Err(
                Fault::Heap(_),
            )) ==> {
                &&& final(self).held_ok()
                &&& final(self).evolved_by(
                    *old(self),
                    |x: usize| -points(path, x) - points(text, x),
                    Set::empty(),
                )
            },
    {
        let ghost s0 = *self;
        let ghost c = s0.held_ok() && s0.holds_strings(path, text);
        if M::CHECKED && !(path.is_ptr() && text.is_ptr()) {
            return Err(Fault::Heap(HeapError::WrongKind));
        }
        self.heap.settle();
        let ghost h1 = self.heap;
        proof {
            lemma_settled_twice(s0.heap);
            assert(h1.words_kept(s0.heap));
            if c {
                s0.lemma_settled_strings(h1, path, text);
            }
        }
        if let Err(e) = self.heap.checked_strings(path, text) {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        let r = host_write_file(
            &self.host,
            self.heap.string_text(path),
            self.heap.string_text(text),
        );
        let Ghost(mid) = self.heap.release_strings(path, text, Ghost(c));
        proof {
            if M::CHECKED {
                lemma_words_trans(self.heap, h1, s0.heap);
                self.lemma_words_kept(s0);
            }
            if c {
                self.lemma_released(s0, h1, mid, path, text);
            }
        }
        r
    }

    /// `Proc.status`。プログラム `program` を引数の列 `args` (引数の数と各引数を、それぞれ NUL で終えて並べたもの) で
    /// 起動し、終了コードを返す。読み終えたら `program`、`args` の順に手放す。子を待つ間に止める語が立てば、子が終わった
    /// 後で `Stopped` を返す。
    ///
    /// # Safety
    /// `string_eq` と同じ (`a` が `program`、`b` が `args`)。
    pub unsafe fn proc_status(&mut self, program: Word, args: Word) -> (r: Result<i64, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(program),
            old(self).heap_spec().word_in(args),
            !M::CHECKED ==> old(self).holds_strings(program, args),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            (r matches Err(Fault::Heap(_))) ==> final(self).heap_spec().settled_heap() == old(
                self,
            ).heap_spec().settled_heap(),
            !M::CHECKED ==> !(r matches Err(Fault::Heap(_))),
            old(self).held_ok() && old(self).holds_strings(program, args) ==> !(r matches Err(
                Fault::Heap(_),
            )) || (r matches Err(Fault::Heap(e)) && chain_err(Err::<(), HeapError>(e))),
            old(self).held_ok() && old(self).holds_strings(program, args) && !(r matches Err(
                Fault::Heap(_),
            )) ==> {
                &&& final(self).held_ok()
                &&& final(self).evolved_by(
                    *old(self),
                    |x: usize| -points(program, x) - points(args, x),
                    Set::empty(),
                )
            },
    {
        let ghost s0 = *self;
        let ghost c = s0.held_ok() && s0.holds_strings(program, args);
        if M::CHECKED && !(program.is_ptr() && args.is_ptr()) {
            return Err(Fault::Heap(HeapError::WrongKind));
        }
        self.heap.settle();
        let ghost h1 = self.heap;
        proof {
            lemma_settled_twice(s0.heap);
            assert(h1.words_kept(s0.heap));
            if c {
                s0.lemma_settled_strings(h1, program, args);
            }
        }
        if let Err(e) = self.heap.checked_strings(program, args) {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        let r = host_run(
            &self.host,
            &self.control,
            self.heap.string_text(program),
            self.heap.string_text(args),
        );
        let Ghost(mid) = self.heap.release_strings(program, args, Ghost(c));
        proof {
            if M::CHECKED {
                lemma_words_trans(self.heap, h1, s0.heap);
                self.lemma_words_kept(s0);
            }
            if c {
                self.lemma_released(s0, h1, mid, program, args);
            }
        }
        r
    }
}

} // verus!
