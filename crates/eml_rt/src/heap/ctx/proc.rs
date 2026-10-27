//! 子のプロセスの extern (docs/spec/object-model.md の「子のプロセス」)。Host に子を起動させ、`Child` のハンドルで
//! 待つ。子の pipe の読み書きは `handle_read` と `handle_write` の `Handle::Child` の枝である。何が起きたかは約束せず
//! (信頼の一覧の T11)、引数と結果の所有と RC の増減を証明する。

#[cfg(verus_only)]
use super::proof::*;
use super::*;
use crate::externs::Fault;
use crate::host::{Child, child_close_stdin, child_wait, host_spawn};

verus! {

impl<M: Mode> Ctx<M> {
    /// 文字列 `a`、`b`、`c`、`d` を受ける extern の前提。呼ぶ側がそれぞれの参照を1つずつ持つ (同じ物体なら、渡した数
    /// だけ持つ)。
    pub open spec fn holds_texts(&self, a: Word, b: Word, c: Word, d: Word) -> bool {
        &&& self.is_string(a)
        &&& self.is_string(b)
        &&& self.is_string(c)
        &&& self.is_string(d)
        &&& self.holds_args(seq![a, b, c, d])
    }

    /// `Proc.spawn_raw`。プログラム `program` を、引数の列 `args`、環境の列 `env`、作業ディレクトリ `cwd` (空なら設定
    /// しない) と、`stdin`、`stdout`、`stderr` の向きで Host に起動させ、`lin` ビットを立てた `Child` の物体を返す。列と
    /// 向きの値は信頼の関数 `host_spawn` が読む。4つの文字列の参照を受け取り、起動できたかどうかによらず手放す。起動
    /// できたときは、`fs_open` と同じく、`Child` を作ってから文字列を手放す。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。4つの語は即値か、このヒープか付けた塊の生きている物体を指し、呼ぶ側がそれぞれの
    /// 参照を1つ持つ (同じ物体なら、渡した数だけ持つ)。速いモードでは、どれも文字列である。検査モードは、Host を呼ぶ前に
    /// 種類と `decref` の前提を1つずつ確かめ、違えば何も起動せずに `WrongKind` などを返す。同じ物体を呼ぶ側の持つ数より
    /// 多く渡したことは、手放すときに見つかる。そのときは、作った `Child` も手放してから `Heap(_)` を返す。
    #[expect(clippy::too_many_arguments, reason = "the seven arguments of `spawn_raw`")]
    pub unsafe fn proc_spawn(
        &mut self,
        program: Word,
        args: Word,
        env: Word,
        cwd: Word,
        stdin: i64,
        stdout: i64,
        stderr: i64,
    ) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(program),
            old(self).heap_spec().word_in(args),
            old(self).heap_spec().word_in(env),
            old(self).heap_spec().word_in(cwd),
            !M::CHECKED ==> old(self).holds_texts(program, args, env, cwd),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            !M::CHECKED ==> !(r matches Err(Fault::Heap(_))),
            old(self).held_ok() && old(self).holds_texts(program, args, env, cwd) && !(
            r matches Err(Fault::Heap(_))) ==> {
                &&& final(self).held_ok()
                &&& final(self).evolved_by(
                    *old(self),
                    |x: usize|
                        -points(program, x) - points(args, x) - points(env, x) - points(cwd, x),
                    Set::empty(),
                )
                &&& r matches Ok(w) ==> {
                    &&& final(self).holds(w)
                    &&& final(self).heap_spec().kind_of(w) == Kind::Foreign
                    &&& final(self).heap_spec().lin_of(w)
                }
            },
    {
        let ghost s0 = *self;
        let ghost ws = seq![program, args, env, cwd];
        let ghost c = s0.held_ok() && s0.holds_texts(program, args, env, cwd);
        if M::CHECKED && !(program.is_ptr() && args.is_ptr() && env.is_ptr() && cwd.is_ptr()) {
            return Err(Fault::Heap(HeapError::WrongKind));
        }
        self.heap.settle();
        let ghost h1 = self.heap;
        let hp = self.heap.header_of(program);
        let ha = self.heap.header_of(args);
        let he = self.heap.header_of(env);
        let hc = self.heap.header_of(cwd);
        proof {
            lemma_settled_twice(s0.heap);
            assert(h1.words_kept(s0.heap));
            h1.lemma_text_spec(program);
            h1.lemma_text_spec(args);
            h1.lemma_text_spec(env);
            h1.lemma_text_spec(cwd);
            s0.heap.lemma_text_spec(program);
            s0.heap.lemma_text_spec(args);
            s0.heap.lemma_text_spec(env);
            s0.heap.lemma_text_spec(cwd);
            if c {
                s0.lemma_holds_one(ws, 0);
                s0.lemma_holds_one(ws, 1);
                s0.lemma_holds_one(ws, 2);
                s0.lemma_holds_one(ws, 3);
                s0.lemma_settled_body(h1, program, Kind::String);
                s0.lemma_settled_body(h1, args, Kind::String);
                s0.lemma_settled_body(h1, env, Kind::String);
                s0.lemma_settled_body(h1, cwd, Kind::String);
            }
        }
        // 確かめはどれも Host を呼ぶ前に済ませ、誤りなら何も起動しない
        if let Err(e) = self.heap.checked_body(hp, Kind::String) {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        if let Err(e) = self.heap.checked_body(ha, Kind::String) {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        if let Err(e) = self.heap.checked_body(he, Kind::String) {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        if let Err(e) = self.heap.checked_body(hc, Kind::String) {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        let spawned = host_spawn(
            &self.host,
            &self.control,
            self.heap.string_text(program),
            self.heap.string_text(args),
            self.heap.string_text(env),
            self.heap.string_text(cwd),
            stdin,
            stdout,
            stderr,
        );
        match spawned {
            Ok(child) => {
                // 文字列を手放す前に `Child` を作る。先に手放すと、空けた文字列の箱を `Child` が使い回しうるので、
                // 呼ぶ前からの変化 (`evolved_by`) を証明で言えない
                let w = self.heap.alloc_foreign(child, true);
                let ghost h2 = self.heap;
                proof {
                    if M::CHECKED {
                        lemma_words_trans(h2, h1, s0.heap);
                        self.lemma_words_kept(s0);
                    }
                    if c {
                        lemma_settle_moved(h1, s0.heap);
                        lemma_alloc_moved(h2, h1, w);
                        lemma_moved_chain(
                            h2,
                            h1,
                            s0.heap,
                            |x: usize| 0int,
                            |x: usize| 0int,
                            |x: usize| 0int,
                            Set::empty(),
                            Set::empty(),
                            Set::empty(),
                        );
                        self.lemma_args_kept(s0, ws);
                    }
                }
                let released = unsafe { self.release_texts(program, args, env, cwd) };
                if let Err(e) = released {
                    // 検査モードだけが来る。作った `Child` も手放し、破棄処理が子を片づける
                    let _ = unsafe { self.release_handle(w) };
                    return Err(e);
                }
                proof {
                    if c {
                        self.lemma_spawned(s0, h1, h2, w, program, args, env, cwd);
                    }
                }
                Ok(w)
            },
            Err(fault) => {
                proof {
                    if c {
                        lemma_settle_moved(h1, s0.heap);
                        self.lemma_args_kept(s0, ws);
                    }
                }
                let released = unsafe { self.release_texts(program, args, env, cwd) };
                released?;
                proof {
                    if c {
                        lemma_moved_chain(
                            self.heap,
                            h1,
                            s0.heap,
                            |x: usize| 0int,
                            |x: usize|
                                -points(program, x) - points(args, x) - points(env, x) - points(
                                    cwd,
                                    x,
                                ),
                            |x: usize|
                                -points(program, x) - points(args, x) - points(env, x) - points(
                                    cwd,
                                    x,
                                ),
                            Set::empty(),
                            Set::empty(),
                            Set::empty(),
                        );
                    }
                }
                Err(fault)
            },
        }
    }

    /// `proc_spawn` が読んだ文字列 `a`、`b`、`c`、`d` を、この順に手放す。検査モードは手放すたびに `decref` の前提を
    /// 確かめるので、同じ物体を呼ぶ側の持つ数より多く渡せば、そこで `Heap(_)` を返す。
    ///
    /// # Safety
    /// `proc_spawn` と同じである。
    unsafe fn release_texts(&mut self, a: Word, b: Word, c: Word, d: Word) -> (r: Result<(), Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(a),
            old(self).heap_spec().word_in(b),
            old(self).heap_spec().word_in(c),
            old(self).heap_spec().word_in(d),
            a.is_ptr_spec(),
            b.is_ptr_spec(),
            c.is_ptr_spec(),
            d.is_ptr_spec(),
            !M::CHECKED ==> old(self).holds_args(seq![a, b, c, d]),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || (r matches Err(Fault::Heap(_))),
            !M::CHECKED ==> r is Ok,
            M::CHECKED ==> final(self).heap_spec().words_kept(old(self).heap_spec()),
            old(self).held_ok() && old(self).holds_args(seq![a, b, c, d]) && r is Ok ==> {
                &&& final(self).held_ok()
                &&& moved(
                    final(self).heap,
                    old(self).heap,
                    |x: usize| -points(a, x) - points(b, x) - points(c, x) - points(d, x),
                    Set::empty(),
                )
            },
    {
        let ghost s0 = *self;
        let ghost ws = seq![a, b, c, d];
        let ghost k = s0.held_ok() && s0.holds_args(ws);
        let ghost d1 = |x: usize| -points(a, x);
        let ghost d2 = |x: usize| -points(a, x) - points(b, x);
        let ghost d3 = |x: usize| -points(a, x) - points(b, x) - points(c, x);
        let ghost d4 = |x: usize| -points(a, x) - points(b, x) - points(c, x) - points(d, x);
        proof {
            if k {
                s0.lemma_holds_one(ws, 0);
            }
        }
        let released = unsafe { self.release_handle(a) };
        released?;
        let ghost s1 = *self;
        proof {
            if k {
                assert forall|x: usize| #[trigger] points_in(ws, x) + d1(x) >= points(ws[1], x) by {
                    lemma_points_in(ws, x);
                }
                self.lemma_kept_one(s0, ws, 1, d1);
            }
        }
        if let Err(e) = unsafe { self.release_handle(b) } {
            proof {
                if M::CHECKED {
                    lemma_words_trans(self.heap, s1.heap, s0.heap);
                }
            }
            return Err(e);
        }
        let ghost s2 = *self;
        proof {
            if M::CHECKED {
                lemma_words_trans(self.heap, s1.heap, s0.heap);
            }
            if k {
                lemma_moved_chain(
                    self.heap,
                    s1.heap,
                    s0.heap,
                    d1,
                    |x: usize| -points(b, x),
                    d2,
                    Set::empty(),
                    Set::empty(),
                    Set::empty(),
                );
                assert forall|x: usize| #[trigger] points_in(ws, x) + d2(x) >= points(ws[2], x) by {
                    lemma_points_in(ws, x);
                }
                self.lemma_kept_one(s0, ws, 2, d2);
            }
        }
        if let Err(e) = unsafe { self.release_handle(c) } {
            proof {
                if M::CHECKED {
                    lemma_words_trans(self.heap, s2.heap, s0.heap);
                }
            }
            return Err(e);
        }
        let ghost s3 = *self;
        proof {
            if M::CHECKED {
                lemma_words_trans(self.heap, s2.heap, s0.heap);
            }
            if k {
                lemma_moved_chain(
                    self.heap,
                    s2.heap,
                    s0.heap,
                    d2,
                    |x: usize| -points(c, x),
                    d3,
                    Set::empty(),
                    Set::empty(),
                    Set::empty(),
                );
                assert forall|x: usize| #[trigger] points_in(ws, x) + d3(x) >= points(ws[3], x) by {
                    lemma_points_in(ws, x);
                }
                self.lemma_kept_one(s0, ws, 3, d3);
            }
        }
        if let Err(e) = unsafe { self.release_handle(d) } {
            proof {
                if M::CHECKED {
                    lemma_words_trans(self.heap, s3.heap, s0.heap);
                }
            }
            return Err(e);
        }
        proof {
            if M::CHECKED {
                lemma_words_trans(self.heap, s3.heap, s0.heap);
            }
            if k {
                lemma_moved_chain(
                    self.heap,
                    s3.heap,
                    s0.heap,
                    d3,
                    |x: usize| -points(d, x),
                    d4,
                    Set::empty(),
                    Set::empty(),
                    Set::empty(),
                );
            }
        }
        Ok(())
    }

    /// `Proc.wait`。`Child` のハンドル `h` の子を、標準入力を閉じてから待ち、終了コードを返す。`h` の参照を受け取り、
    /// 待てたかどうかによらず手放す。待ち終えた `Child` の破棄処理は子を kill しない。待つ間に止める語が立てば、子が
    /// 終わった後で `Stopped` を返す。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。`h` は即値か、このヒープか付けた塊の生きている物体を指し、呼ぶ側がその参照を
    /// 1つ持つ。速いモードでは、`h` は局所で rc が 1 である (検査モードは確かめ、違えば `NotUnique` か `WrongOwner` を
    /// 返す)。`Child` でなければ、どちらのモードでも `WrongKind` を返す。
    pub unsafe fn proc_wait(&mut self, h: Word) -> (r: Result<i64, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(h),
            !M::CHECKED ==> old(self).holds(h) && old(self).heap_spec().rc_value(h) == 1,
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            !M::CHECKED && (r matches Err(Fault::Heap(_))) ==> r == Err::<i64, Fault>(
                Fault::Heap(HeapError::WrongKind),
            ) && *final(self) == *old(self),
            old(self).held_ok() && old(self).holds(h) && old(self).heap_spec().rc_value(h) == 1
                && !(r matches Err(Fault::Heap(_))) ==> {
                &&& final(self).held_ok()
                &&& final(self).evolved_by(*old(self), |x: usize| -points(h, x), Set::empty())
            },
    {
        let ghost s0 = *self;
        if !h.is_ptr() {
            return Err(Fault::Heap(HeapError::WrongKind));
        }
        proof {
            if s0.held_ok() && s0.holds(h) {
                s0.lemma_held_arg(h);
            }
        }
        let p = match unsafe { self.heap.foreign_unique::<Child>(h) } {
            Ok(p) => p,
            Err(e) => {
                return Err(Fault::Heap(e));
            },
        };
        let waited = unsafe { child_wait(p, &self.control) };
        let released = unsafe { self.release_handle(h) };
        released?;
        waited
    }

    /// `Proc.close_stdin`。`Child` のハンドル `h` の子の標準入力を閉じ、`h` をそのまま返す。ヒープは変わらない。
    ///
    /// # Safety
    /// `proc_wait` と同じである。
    pub unsafe fn proc_close_stdin(&mut self, h: Word) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(h),
            !M::CHECKED ==> old(self).holds(h) && old(self).heap_spec().rc_value(h) == 1,
        ensures
            *final(self) == *old(self),
            r is Ok || (r matches Err(Fault::Heap(_))),
            r matches Ok(x) ==> x == h,
            !M::CHECKED && r is Err ==> r == Err::<Word, Fault>(Fault::Heap(HeapError::WrongKind)),
    {
        let ghost s0 = *self;
        if !h.is_ptr() {
            return Err(Fault::Heap(HeapError::WrongKind));
        }
        proof {
            if s0.held_ok() && s0.holds(h) {
                s0.lemma_held_arg(h);
            }
        }
        match unsafe { self.heap.foreign_unique::<Child>(h) } {
            Ok(p) => {
                unsafe { child_close_stdin(p) };
                Ok(h)
            },
            Err(e) => Err(Fault::Heap(e)),
        }
    }

    /// 呼ぶ側が `ws` と合わせて持つ語は、どれも1つずつ持たれている。
    proof fn lemma_holds_one(&self, ws: Seq<Word>, i: int)
        requires
            self.held_ok(),
            self.holds_args(ws),
            0 <= i < ws.len(),
        ensures
            self.holds(ws[i]),
            self.heap.word_in(ws[i]),
    {
        let w = ws[i];
        let ws1 = seq![w];
        assert(held_word(self.heap, ws[i]));
        assert(ws1[0] == w);
        lemma_moved_refl(self.heap);
        assert forall|x: usize| #[trigger] self.need(ws1, x) > 0 implies self.need(ws, x) + 0int
            >= self.need(ws1, x) by {
            lemma_points_in(ws1, x);
            lemma_points_at(ws, i, x);
        }
        self.lemma_holds_moved(*self, ws, ws1, |x: usize| 0int, Set::empty());
    }

    /// 呼ぶ前の文脈 `s0` から `moved(d)` で変えた後、`ws` の `i` 番の語の参照が `d` で減っていなければ、呼ぶ側はそれを
    /// まだ持つ。
    proof fn lemma_kept_one(&self, s0: Ctx<M>, ws: Seq<Word>, i: int, d: spec_fn(usize) -> int)
        requires
            s0.held_ok(),
            s0.holds_args(ws),
            0 <= i < ws.len(),
            self.held_ok(),
            moved(self.heap, s0.heap, d, Set::empty()),
            self.evv == s0.evv,
            self.yielding == s0.yielding,
            forall|x: usize| #[trigger] points_in(ws, x) + d(x) >= points(ws[i], x),
        ensures
            self.holds(ws[i]),
            self.heap.word_in(ws[i]),
    {
        let w = ws[i];
        let ws1 = seq![w];
        assert(held_word(s0.heap, ws[i]));
        assert(ws1[0] == w);
        assert forall|x: usize| #[trigger] self.need(ws1, x) > 0 implies s0.need(ws, x) + d(x)
            >= self.need(ws1, x) by {
            lemma_points_in(ws1, x);
            assert(points_in(ws, x) + d(x) >= points(ws[i], x));
        }
        self.lemma_holds_moved(s0, ws, ws1, d, Set::empty());
        assert(held_word(self.heap, ws1[0]));
    }

    /// 呼ぶ前の文脈 `s0` から、物体を空けずに参照の数を変えない変化 (`settle` と確保) の後も、呼ぶ側は `ws` の語を
    /// 持ち、モデルは合っている。
    proof fn lemma_args_kept(&self, s0: Ctx<M>, ws: Seq<Word>)
        requires
            s0.held_ok(),
            s0.holds_args(ws),
            self.heap.consistent(),
            self.heap.settleable(),
            moved(self.heap, s0.heap, |x: usize| 0int, Set::empty()),
            self.evv == s0.evv,
            self.yielding == s0.yielding,
            self.max_evv == s0.max_evv,
        ensures
            self.held_ok(),
            self.holds_args(ws),
            forall|i: int| 0 <= i < ws.len() ==> self.heap.word_in(#[trigger] ws[i]),
    {
        let z = |x: usize| 0int;
        let none = Set::<usize>::empty();
        assert forall|x: usize| #[trigger] self.need(ws, x) > 0 implies s0.need(ws, x) + z(x)
            >= self.need(ws, x) by {}
        self.lemma_holds_moved(s0, ws, ws, z, none);
        assert forall|x: usize| #[trigger] self.need(seq![], x) > 0 implies s0.need(ws, x) + z(x)
            >= self.need(seq![], x) by {
            lemma_points_in(seq![], x);
            lemma_points_at(ws, 0, x);
        }
        self.lemma_holds_moved(s0, ws, seq![], z, none);
        self.lemma_evv_kept(s0, z, none);
        assert forall|i: int| 0 <= i < ws.len() implies self.heap.word_in(#[trigger] ws[i]) by {
            assert(held_word(self.heap, ws[i]));
        }
    }

    /// `proc_spawn` の、起動できた道でモデルが合っている道。`h1` は `settle` の後、`h2` は `Child` の物体 `w` を作った
    /// 後で、その後に4つの文字列を手放した。
    proof fn lemma_spawned(
        &self,
        s0: Ctx<M>,
        h1: Heap<M>,
        h2: Heap<M>,
        w: Word,
        a: Word,
        b: Word,
        c: Word,
        d: Word,
    )
        requires
            s0.held_ok(),
            h1 == s0.heap.settled_heap(),
            h1.consistent(),
            live(h1, a.bits_spec()),
            live(h1, b.bits_spec()),
            live(h1, c.bits_spec()),
            live(h1, d.bits_spec()),
            h2.made(h1, w),
            h2.owned(w),
            h2.kind_of(w) == Kind::Foreign,
            h2.lin_of(w),
            moved(
                self.heap,
                h2,
                |x: usize| -points(a, x) - points(b, x) - points(c, x) - points(d, x),
                Set::empty(),
            ),
            self.held_ok(),
            self.evv == s0.evv,
            self.yielding == s0.yielding,
        ensures
            self.evolved_by(
                s0,
                |x: usize| -points(a, x) - points(b, x) - points(c, x) - points(d, x),
                Set::empty(),
            ),
            self.holds(w),
            self.heap.kind_of(w) == Kind::Foreign,
            self.heap.lin_of(w),
    {
        let h0 = s0.heap;
        let z = |x: usize| 0int;
        let d4 = |x: usize| -points(a, x) - points(b, x) - points(c, x) - points(d, x);
        let none = Set::<usize>::empty();
        lemma_settle_moved(h1, h0);
        lemma_alloc_moved(h2, h1, w);
        lemma_moved_chain(h2, h1, h0, z, z, z, none, none, none);
        lemma_moved_chain(self.heap, h2, h0, z, d4, d4, none, none, none);
        assert(d4(w.bits_spec()) == 0);
        lemma_arg_moved(self.heap, h2, d4, none, w);
        lemma_view_live(self.heap, w.bits_spec());
        assert(!live(h0, w.bits_spec()));
        self.lemma_fresh_holds(s0, w);
        assert(same_head(self.heap, h2, w.bits_spec()));
    }
}

} // verus!
