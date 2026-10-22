//! 出力とファイルの extern (docs/spec/object-model.md の「extern」)。Host を通して書き、開き、読む。何が書かれ、
//! 何が読めたかは約束せず (信頼の一覧の T11)、引数と結果の所有と RC の増減を証明する。

use super::positions::find_byte;
#[cfg(verus_only)]
use super::proof::*;
#[cfg(verus_only)]
use super::strings::lemma_pushed_moved;
use super::*;
use crate::alloc::too_large;
use crate::externs::{Fault, Form, Handle, IoOp, Open, ReadAmount, StdStream, Write};
use crate::host::{
    FileIn, FileOut, OutputHandle, Source, StderrHandle, StdinHandle, StdoutHandle, Stream,
    consume, file_out_flush, file_out_write, fill, host_flush, host_open, host_open_write,
    host_write, host_write_line, open_stdin, open_stream, read_text,
};
#[cfg(verus_only)]
use vstd::string::StringSliceAdditionalSpecFns;

verus! {

/// `handle_write` が型を確かめたハンドルの書き込み先。標準のストリームは Host の buffer に、`FileOut` は自分の buffer
/// に書く。
enum Sink {
    Stream(Stream),
    File(*mut FileOut),
}

/// 行の読みの途中のヒープ `h`。呼ぶ前のヒープ `h0` からは、新しい物体を作って手放しただけで、`h0` で生きていた物体は
/// みな同じブロックのまま生きている。`w` はその間に作った、伸ばせるバイト列である。
spec fn line_held<M: Mode>(h: Heap<M>, h0: Heap<M>, w: Word) -> bool {
    &&& h.consistent()
    &&& h.settleable()
    &&& h.decref_ok(w)
    &&& h.readable(w)
    &&& h.kind_of(w) == Kind::Bytes
    &&& moved(h, h0, |x: usize| 0int, Set::empty())
    &&& no_free(h, h0)
    &&& w.is_ptr_spec()
    &&& !live(h0, w.bits_spec())
    &&& h.core.settled_view().ext(w.bits_spec()) >= 1
}

/// `alloc_bytes` で行の最初の部分を作った後。
proof fn lemma_line_made<M: Mode>(h1: Heap<M>, h0: Heap<M>, w: Word)
    requires
        h0.consistent(),
        h0.settleable(),
        h1.made(h0, w),
        h1.owned(w),
        h1.is_bytes(w),
    ensures
        line_held(h1, h0, w),
{
    lemma_alloc_moved(h1, h0, w);
    lemma_view_live(h1, w.bits_spec());
    h1.lemma_text_spec(w);
}

/// `bytes_extend` で行を伸ばした後。
proof fn lemma_line_extended<M: Mode>(h2: Heap<M>, h1: Heap<M>, h0: Heap<M>, w: Word, w2: Word)
    requires
        line_held(h1, h0, w),
        h2.consistent(),
        h2.pushed(h1, w, w2),
        h2.others_kept(h1, w),
        h2.is_bytes(w2),
    ensures
        line_held(h2, h0, w2),
{
    let z = |x: usize| 0int;
    let d2 = |x: usize| points(w2, x) - points(w, x);
    lemma_pushed_moved(h2, h1, w, w2);
    h2.lemma_pushed(h1, w, w2);
    let v0 = h0.core.settled_view();
    assert forall|b: usize| #[trigger] live(h0, b) implies live(h2, b) && h2.core.block(b)
        == h0.core.block(b) by {
        assert(live(h1, b));
        assert(b != w.bits_spec());
        assert(h1.core.has(b));
    }
    assert forall|b: usize|
        #![trigger live(h2, b), live(h0, b)]
        live(h0, b) && live(h2, b) implies same_head(h2, h0, b) && h2.core.perm(b).slots
        == h0.core.perm(b).slots by {
        assert(live(h1, b));
        assert(b != w.bits_spec());
    }
    assert forall|b: usize|
        #![trigger live(h0, b)]
        live(h0, b) && (v0.rc_of(b) == 0 || v0.ext(b) + z(b) >= 1) implies {
        &&& live(h2, b)
        &&& h2.core.block(b) == h0.core.block(b)
        &&& v0.rc_of(b) != 0 ==> h2.core.settled_view().ext(b) == v0.ext(b) + z(b)
    } by {
        assert(live(h1, b));
        assert(b != w.bits_spec());
        assert(same_head(h1, h0, b));
        assert(d2(b) == 0) by {
            if w2 != w {
                assert(!live(h1, w2.bits_spec()));
            }
        }
    }
    h2.core.lemma_settled_view();
    lemma_view_live(h2, w2.bits_spec());
    h2.lemma_text_spec(w2);
}

impl<M: Mode> Ctx<M> {
    /// `println` と `eprintln`。文字列 `s` と `\n` を `stream` に書く。`s` の参照を受け取り、書けたかどうかによらず手放す。
    /// Host が出力をまとめて書くときは、`Err(Output)` は、この行かそれより前の行を書けなかったことを表す。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。`s` は即値か、このヒープか付けた塊の生きている物体を指し、呼ぶ側がその参照を1つ
    /// 持つ。速いモードでは、`s` は文字列である (検査モードは確かめ、違えば何も書かずに `WrongKind` を返す)。
    pub unsafe fn write_string(&mut self, stream: Stream, s: Word) -> (r: Result<(), Fault>)
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
                s0.lemma_settled_body(h1, s, Kind::String);
            }
        }
        // 確かめはどれも Host を呼ぶ前に済ませ、誤りなら何も書かない
        if let Err(e) = self.heap.checked_body(hs, Kind::String) {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        let written = host_write_line(&self.host, stream, self.heap.string_text(s));
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
                s0.lemma_settled_body(h1, s, Kind::String);
            }
        }
        if let Err(e) = self.heap.checked_body(hs, Kind::String) {
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

    /// `FileIn.open`、`FileOut.create`、`FileOut.append`。パス `path` のファイルを `open` の向きで Host に開かせ、`lin`
    /// ビットを立てた物体 (`Open::Read` なら `FileIn`、`Open::Write` なら `FileOut`) を返す。`path` の参照を受け取り、
    /// 開けたかどうかによらず手放す。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。`path` は即値か、このヒープか付けた塊の生きている物体を指し、呼ぶ側がその参照を
    /// 1つ持つ。速いモードでは、`path` は文字列である (検査モードは確かめ、違えば何も開かずに `WrongKind` を返す)。
    pub unsafe fn fs_open(&mut self, path: Word, open: Open) -> (r: Result<Word, Fault>)
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
                s0.lemma_settled_body(h1, path, Kind::String);
            }
        }
        // 確かめはどれも Host を呼ぶ前に済ませ、誤りなら何も開かない
        if let Err(e) = self.heap.checked_body(hp, Kind::String) {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        let made = match open {
            // パスを手放す前にハンドルを作る。先に手放すと、空けたパスの箱をハンドルが使い回しうるので、呼ぶ前からの
            // 変化 (`evolved_by`) を証明で言えない
            Open::Read => match host_open(&self.host, self.heap.string_text(path)) {
                Ok(file) => Ok(self.heap.alloc_foreign(file, true)),
                Err(e) => Err(e),
            },
            Open::Write(mode) => match host_open_write(
                &self.host,
                self.heap.string_text(path),
                mode,
            ) {
                Ok(file) => Ok(self.heap.alloc_foreign(file, true)),
                Err(e) => Err(e),
            },
        };
        match made {
            Ok(f) => {
                let ghost h2 = self.heap;
                proof {
                    self.lemma_room_made(h1, path, f, Kind::String);
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

    /// ハンドル `h` を借りて読む。`read` が読む量を、`form` が結果の形を決める。結果は新しい物体で、組は作らない。
    /// 塊の読みだけは、受け取った `buf` の後ろに足して返し、`form` を見ない。読みは 64 KiB ずつで、区切りごとに止める
    /// 語と確保量を見る (docs/spec/object-model.md の「読み手」)。`h` の参照は呼ぶ側に残り、参照の数は成功でも誤りでも
    /// 変わらない。読みの誤り、UTF-8 でない中身、止めたときもハンドルを手放さない。そのとき、バイト列の行の読みは途中まで
    /// 読んだ行を、塊の読みは `buf` を手放し、ほかの読みはヒープを変えない。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。`h` は即値か、このヒープか付けた塊の生きている物体を指し、呼ぶ側がその参照を
    /// 1つ持つ。速いモードでは、`h` は局所で rc が 1 である (検査モードは確かめ、違えば `NotUnique` か `WrongOwner`
    /// を返す)。`handle` の型のハンドルでなければ、どちらのモードでも `WrongKind` を返す。塊の読みの `buf` も即値か
    /// 生きている物体を指し、呼ぶ側がその参照を1つ持つ。速いモードでは、`buf` はバイト列である (検査モードは確かめ、
    /// 違えば `WrongKind` を返す)。
    pub unsafe fn handle_read(
        &mut self,
        h: Word,
        handle: Handle,
        read: ReadAmount,
        form: Form,
    ) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(h),
            !M::CHECKED ==> old(self).holds(h) && old(self).heap_spec().rc_value(h) == 1,
            read matches ReadAmount::Chunk(buf) ==> old(self).heap_spec().word_in(buf) && (
            !M::CHECKED ==> old(self).is_bytes(buf) && old(self).holds(buf)),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            !M::CHECKED && (r matches Err(Fault::Heap(_))) ==> r == Err::<Word, Fault>(
                Fault::Heap(HeapError::WrongKind),
            ) && *final(self) == *old(self),
            r is Err && (read == ReadAmount::All || read == ReadAmount::Line && form == Form::Text)
                ==> *final(self) == *old(self),
            r matches Ok(s) ==> if read is Chunk || form == Form::Bytes {
                final(self).is_bytes(s)
            } else {
                final(self).is_string(s)
            },
            !(read is Chunk) && old(self).held_ok() && old(self).holds(h) && old(
                self,
            ).heap_spec().rc_value(h) == 1 && !(r matches Err(Fault::Heap(_))) ==> {
                &&& final(self).held_ok()
                &&& final(self).evolved_by(*old(self), |x: usize| 0int, Set::empty())
                &&& r matches Ok(s) ==> final(self).holds(s)
            },
            read matches ReadAmount::Chunk(buf) ==> (old(self).held_ok() && old(self).holds(h)
                && old(self).heap_spec().rc_value(h) == 1 && old(self).is_bytes(buf) && old(
                self,
            ).holds(buf) && !(r matches Err(Fault::Heap(_))) ==> {
                &&& final(self).held_ok()
                &&& r matches Ok(x) ==> final(self).holds(x) && final(self).evolved_by(
                    *old(self),
                    |y: usize| points(x, y) - points(buf, y),
                    Set::empty().insert(buf.bits_spec()),
                )
                &&& r is Err ==> final(self).evolved_by(
                    *old(self),
                    |y: usize| -points(buf, y),
                    Set::empty(),
                )
            }),
    {
        let ghost s0 = *self;
        if !h.is_ptr() {
            return Err(Fault::Heap(HeapError::WrongKind));
        }
        proof {
            if s0.held_ok() && s0.holds(h) && s0.heap.rc_value(h) == 1 {
                s0.lemma_held_arg(h);
            }
        }
        match handle {
            Handle::FileIn => match unsafe { self.heap.foreign_unique::<FileIn>(h) } {
                Ok(p) => unsafe { self.read_from(p, false, read, form) },
                Err(e) => Err(Fault::Heap(e)),
            },
            // 標準入力の補充の前には、プロンプトが読む前に出るよう標準出力を書き出す (docs/spec/object-model.md の
            // 「出力のバッファ」)
            Handle::Stdin => match unsafe { self.heap.foreign_unique::<StdinHandle>(h) } {
                Ok(p) => unsafe { self.read_from(p, true, read, form) },
                Err(e) => Err(Fault::Heap(e)),
            },
            // 書き込みのハンドルは読めない
            Handle::Stdout | Handle::Stderr | Handle::FileOut => Err(
                Fault::Heap(HeapError::WrongKind),
            ),
        }
    }

    /// `handle_read` の、借りたハンドルの中身 `p` からの読み。`flush` なら、補充の前ごとに Host の出力を書き出させる。
    ///
    /// # Safety
    /// `p` は `foreign_unique` で借りたハンドルの中身を指し、読む間ほかに参照がない。塊の読みの `buf` は `handle_read`
    /// と同じである。
    unsafe fn read_from<S: Source>(
        &mut self,
        p: *mut S,
        flush: bool,
        read: ReadAmount,
        form: Form,
    ) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
            read matches ReadAmount::Chunk(buf) ==> old(self).heap_spec().word_in(buf) && (
            !M::CHECKED ==> old(self).is_bytes(buf) && old(self).holds(buf)),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            !M::CHECKED ==> !(r matches Err(Fault::Heap(_))),
            r is Err && (read == ReadAmount::All || read == ReadAmount::Line && form == Form::Text)
                ==> *final(self) == *old(self),
            r matches Ok(s) ==> if read is Chunk || form == Form::Bytes {
                final(self).is_bytes(s)
            } else {
                final(self).is_string(s)
            },
            !(read is Chunk) && old(self).held_ok() && !(r matches Err(Fault::Heap(_))) ==> {
                &&& final(self).held_ok()
                &&& final(self).evolved_by(*old(self), |x: usize| 0int, Set::empty())
                &&& r matches Ok(s) ==> final(self).holds(s)
            },
            read matches ReadAmount::Chunk(buf) ==> (old(self).held_ok() && old(self).is_bytes(buf)
                && old(self).holds(buf) && !(r matches Err(Fault::Heap(_))) ==> {
                &&& final(self).held_ok()
                &&& r matches Ok(x) ==> final(self).holds(x) && final(self).evolved_by(
                    *old(self),
                    |y: usize| points(x, y) - points(buf, y),
                    Set::empty().insert(buf.bits_spec()),
                )
                &&& r is Err ==> final(self).evolved_by(
                    *old(self),
                    |y: usize| -points(buf, y),
                    Set::empty(),
                )
            }),
    {
        match read {
            ReadAmount::All => unsafe { self.read_whole(p, flush, false, form) },
            ReadAmount::Line => match form {
                Form::Text => unsafe { self.read_whole(p, flush, true, Form::Text) },
                Form::Bytes => unsafe { self.read_line_bytes(p, flush) },
            },
            ReadAmount::Chunk(buf) => unsafe { self.read_chunk(p, flush, buf) },
        }
    }

    /// `handle_read` の全部の読みと、文字列の行の読み。`read_text` で Rust の `Vec` に読み、ヒープの物体に1回写す。
    /// どの誤りもヒープを変えない。
    ///
    /// # Safety
    /// `p` は `foreign_unique` で借りたハンドルの中身を指し、読む間ほかに参照がない。
    unsafe fn read_whole<S: Source>(
        &mut self,
        p: *mut S,
        flush: bool,
        line: bool,
        form: Form,
    ) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Err ==> *final(self) == *old(self) && !(r matches Err(Fault::Heap(_))),
            r matches Ok(s) ==> if form == Form::Bytes {
                final(self).is_bytes(s)
            } else {
                final(self).is_string(s)
            },
            r matches Ok(s) ==> (old(self).held_ok() ==> {
                &&& final(self).held_ok()
                &&& final(self).holds(s)
                &&& final(self).evolved_by(*old(self), |x: usize| 0int, Set::empty())
            }),
    {
        let ghost s0 = *self;
        let bytes = (unsafe { read_text(p, &self.control, self.flushed_host(flush), line, form) })?;
        let s = match form {
            Form::Text => {
                let text = unsafe { str::from_utf8_unchecked(bytes.as_slice()) };
                self.heap.alloc_string(text)
            },
            Form::Bytes => self.heap.alloc_bytes(bytes.as_slice()),
        };
        if M::CHECKED {
            self.stats.string_bytes_copied = self.stats.string_bytes_copied.wrapping_add(
                bytes.len() as u64,
            );
        }
        proof {
            if M::CHECKED {
                self.lemma_words_kept(s0);
            }
            if s0.held_ok() {
                self.lemma_read_lent(s0, s);
            }
        }
        Ok(s)
    }

    /// `handle_read` のバイト列の行の読み。借りた読み手の buffer の slice を `alloc_bytes` と `bytes_extend` にそのまま
    /// 渡すので、写しは1行に1回である。読みの誤りと止めたときは、途中まで読んだ行を手放す。
    ///
    /// # Safety
    /// `p` は `foreign_unique` で借りたハンドルの中身を指し、読む間ほかに参照がない。
    unsafe fn read_line_bytes<S: Source>(&mut self, p: *mut S, flush: bool) -> (r: Result<
        Word,
        Fault,
    >)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            !M::CHECKED ==> !(r matches Err(Fault::Heap(_))),
            r matches Ok(w) ==> final(self).is_bytes(w),
            old(self).held_ok() && !(r matches Err(Fault::Heap(_))) ==> {
                &&& final(self).held_ok()
                &&& final(self).evolved_by(*old(self), |x: usize| 0int, Set::empty())
                &&& r matches Ok(w) ==> final(self).holds(w)
            },
    {
        let ghost s0 = *self;
        let ghost h0 = self.heap;
        let ghost c = s0.held_ok();
        let mut acc = Word::UNIT;
        let mut have = false;
        let mut total: usize = 0;
        loop
            invariant
                self.wf(),
                s0.wf(),
                s0 == *old(self),
                h0 == s0.heap,
                c == s0.held_ok(),
                self.evv == s0.evv,
                self.yielding == s0.yielding,
                self.max_evv == s0.max_evv,
                !M::CHECKED ==> c,
                M::CHECKED ==> self.heap.words_kept(h0),
                !have ==> self.heap == h0 && total == 0,
                have ==> acc.is_ptr_spec() && self.heap.word_in(acc) && self.heap.is_bytes(acc),
                have && c ==> line_held(self.heap, h0, acc),
            decreases usize::MAX - total,
        {
            let buf = match unsafe { fill(p, &self.control, self.flushed_host(flush)) } {
                Ok(buf) => buf,
                Err(fault) => {
                    if have {
                        let ghost h1 = self.heap;
                        if let Err(e) = unsafe { self.heap.decref(acc) } {
                            proof {
                                lemma_words_trans(self.heap, h1, h0);
                                self.lemma_words_kept(s0);
                            }
                            return Err(Fault::Heap(e));
                        }
                        proof {
                            if M::CHECKED {
                                lemma_words_trans(self.heap, h1, h0);
                                self.lemma_words_kept(s0);
                            }
                            if c {
                                self.lemma_line_dropped(s0, h1, acc);
                            }
                        }
                    } else {
                        proof {
                            if c {
                                lemma_moved_refl(h0);
                                self.lemma_line_ok(s0);
                            }
                        }
                    }
                    return Err(fault);
                },
            };
            let len = buf.len();
            if len == 0 {
                if have {
                    if M::CHECKED {
                        self.stats.string_bytes_copied
                            = self.stats.string_bytes_copied.wrapping_add(total as u64);
                    }
                    proof {
                        if c {
                            self.lemma_line_done(s0, acc);
                        }
                    }
                    return Ok(acc);
                }
                let empty: &[u8] = &[];
                let w = self.heap.alloc_bytes(empty);
                proof {
                    if M::CHECKED {
                        self.lemma_words_kept(s0);
                    }
                    if c {
                        lemma_line_made(self.heap, h0, w);
                        self.lemma_line_done(s0, w);
                    }
                }
                return Ok(w);
            }
            let (n, done) = match find_byte(buf, 0, b'\n') {
                Some(i) => (i + 1, true),
                None => (len, false),
            };
            let part = vstd::slice::slice_subrange(buf, 0, n);
            let next = match total.checked_add(n) {
                Some(t) => t,
                None => too_large(),
            };
            let ghost h1 = self.heap;
            if !have {
                acc = self.heap.alloc_bytes(part);
                have = true;
                proof {
                    if M::CHECKED {
                        self.lemma_words_kept(s0);
                    }
                    if c {
                        lemma_line_made(self.heap, h0, acc);
                    }
                }
            } else {
                proof {
                    if c {
                        self.heap.lemma_push_ok(acc, Kind::Bytes);
                    }
                }
                let w = match unsafe { self.heap.bytes_extend(acc, part) } {
                    Ok(w) => w,
                    Err(e) => {
                        proof {
                            lemma_words_trans(self.heap, h1, h0);
                            self.lemma_words_kept(s0);
                        }
                        return Err(Fault::Heap(e));
                    },
                };
                proof {
                    if M::CHECKED {
                        lemma_words_trans(self.heap, h1, h0);
                        self.lemma_words_kept(s0);
                    }
                    if c {
                        lemma_line_extended(self.heap, h1, h0, acc, w);
                    }
                }
                acc = w;
            }
            proof {
                if c {
                    self.lemma_line_ok(s0);
                }
            }
            total = next;
            unsafe { consume(p, n) };
            if done {
                if M::CHECKED {
                    self.stats.string_bytes_copied = self.stats.string_bytes_copied.wrapping_add(
                        total as u64,
                    );
                }
                proof {
                    if c {
                        self.lemma_line_done(s0, acc);
                    }
                }
                return Ok(acc);
            }
        }
    }

    /// `handle_read` の塊の読み。読み手の buffer に残ったバイトを、空なら1回補充して読めたバイトを、`buf` の後ろに足す
    /// (`Bytes.push` と同じく、一意で空きがあればその場で足す)。`buf` の参照を受け取り、読みの誤りと止めたときは
    /// 手放す。検査モードの確かめは補充の前に済ませ、足した後で読み手のバイトを使ったことにする。
    ///
    /// # Safety
    /// `p` は `foreign_unique` で借りたハンドルの中身を指し、読む間ほかに参照がない。`buf` は即値か、このヒープか付けた
    /// 塊の生きている物体を指し、呼ぶ側がその参照を1つ持つ。速いモードでは、`buf` はバイト列である。
    unsafe fn read_chunk<S: Source>(&mut self, p: *mut S, flush: bool, buf: Word) -> (r: Result<
        Word,
        Fault,
    >)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(buf),
            !M::CHECKED ==> old(self).is_bytes(buf) && old(self).holds(buf),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            !M::CHECKED ==> !(r matches Err(Fault::Heap(_))),
            r matches Ok(x) ==> final(self).is_bytes(x),
            old(self).held_ok() && old(self).is_bytes(buf) && old(self).holds(buf) && !(
            r matches Err(Fault::Heap(_))) ==> {
                &&& final(self).held_ok()
                &&& r matches Ok(x) ==> final(self).holds(x) && final(self).evolved_by(
                    *old(self),
                    |y: usize| points(x, y) - points(buf, y),
                    Set::empty().insert(buf.bits_spec()),
                )
                &&& r is Err ==> final(self).evolved_by(
                    *old(self),
                    |y: usize| -points(buf, y),
                    Set::empty(),
                )
            },
    {
        let ghost s0 = *self;
        let ghost c = s0.held_ok() && s0.is_bytes(buf) && s0.holds(buf);
        if M::CHECKED && !buf.is_ptr() {
            return Err(Fault::Heap(HeapError::WrongKind));
        }
        self.heap.settle();
        let ghost h1 = self.heap;
        let hb = self.heap.header_of(buf);
        proof {
            lemma_settled_twice(s0.heap);
            assert(h1.words_kept(s0.heap));
            h1.lemma_text_spec(buf);
            s0.heap.lemma_text_spec(buf);
            if c {
                s0.lemma_settled_body(h1, buf, Kind::Bytes);
                s0.lemma_settled_push_ok(h1, buf, Kind::Bytes);
            }
        }
        // 検査モードの確かめは、Host を呼ぶ前に済ませる (docs/spec/object-model.md の「誤りのときの約束」)。写す
        // バイトを数えるために、一意でない `buf` の長さもここで読む (docs/spec/object-model.md の「仕事の回数」)
        let shared_len = if M::CHECKED {
            if let Err(e) = self.heap.checked_body(hb, Kind::Bytes) {
                proof {
                    self.lemma_words_kept(s0);
                }
                return Err(Fault::Heap(e));
            }
            if self.heap.core.rc_get(hb) == 1 {
                0
            } else {
                self.heap.body_of(buf).len() as u64
            }
        } else {
            0
        };
        let part = match unsafe { fill(p, &self.control, self.flushed_host(flush)) } {
            Ok(part) => part,
            Err(fault) => {
                self.heap.release_text(hb, Ghost(buf), Ghost(c));
                proof {
                    if M::CHECKED {
                        lemma_words_trans(self.heap, h1, s0.heap);
                        self.lemma_words_kept(s0);
                    }
                    if c {
                        self.lemma_printed(s0, h1, buf);
                    }
                }
                return Err(fault);
            },
        };
        let n = part.len();
        let x = match unsafe { self.heap.bytes_extend(buf, part) } {
            Ok(x) => x,
            Err(e) => {
                proof {
                    self.lemma_words_kept(s0);
                }
                return Err(Fault::Heap(e));
            },
        };
        unsafe { consume(p, n) };
        if M::CHECKED {
            self.stats.string_bytes_copied = self.stats.string_bytes_copied.wrapping_add(
                shared_len.wrapping_add(n as u64),
            );
        }
        proof {
            self.heap.lemma_text_spec(x);
            if M::CHECKED {
                lemma_words_trans(self.heap, h1, s0.heap);
                self.lemma_words_kept(s0);
            }
            if c {
                self.lemma_pushed_ok(s0, h1, seq![buf], buf, x, Kind::Bytes);
            }
        }
        Ok(x)
    }

    /// `Stdin.open`、`Stdout.open`、`Stderr.open`。`handle` の型のハンドルを開き、`lin` ビットを立てた `Foreign` の
    /// 物体を返す。生きているハンドルがすでにあれば、何も作らずに `InUse` を返す (docs/spec/stdlib.md の「ハンドル」)。
    /// `FileIn` と `FileOut` はパスを受ける `fs_open` で開く。
    pub fn handle_open(&mut self, handle: Handle) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
            handle != Handle::FileIn,
            handle != Handle::FileOut,
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || r matches Err(Fault::InUse(_)),
            r is Err ==> *final(self) == *old(self),
            r matches Ok(w) ==> final(self).heap_spec().kind_of(w) == Kind::Foreign
                && final(self).heap_spec().lin_of(w),
            r matches Ok(w) ==> (old(self).held_ok() ==> {
                &&& final(self).held_ok()
                &&& final(self).holds(w)
                &&& final(self).evolved_by(*old(self), |x: usize| 0int, Set::empty())
            }),
    {
        let ghost s0 = *self;
        let w = match handle {
            Handle::Stdin => {
                let stdin = open_stdin(&self.host)?;
                self.heap.alloc_foreign(stdin, true)
            },
            Handle::Stdout => {
                match open_stream(&self.host, Stream::Out)? {
                    OutputHandle::Stdout(out) => self.heap.alloc_foreign(out, true),
                    // `open_stream` の約束により来ない
                    OutputHandle::Stderr(_) => {
                        proof {
                            assert(false);
                        }
                        return Err(Fault::InUse(StdStream::Stdout));
                    },
                }
            },
            Handle::Stderr => {
                match open_stream(&self.host, Stream::Err)? {
                    OutputHandle::Stderr(err) => self.heap.alloc_foreign(err, true),
                    // `open_stream` の約束により来ない
                    OutputHandle::Stdout(_) => {
                        proof {
                            assert(false);
                        }
                        return Err(Fault::InUse(StdStream::Stderr));
                    },
                }
            },
            // 前提により来ない
            Handle::FileIn | Handle::FileOut => {
                proof {
                    assert(false);
                }
                return Err(Fault::InUse(StdStream::Stdin));
            },
        };
        proof {
            if M::CHECKED {
                self.lemma_words_kept(s0);
            }
            if s0.held_ok() {
                self.lemma_made_ok(s0, w);
            }
        }
        Ok(w)
    }

    /// `FileIn.close`、`FileOut.close_raw`、`Stdin.close`、`Stdout.close`、`Stderr.close`。ハンドル `h` の参照を手放す。
    /// 最後の参照なので、破棄処理がハンドルを閉じる。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。`h` は即値か、このヒープか付けた塊の生きている物体を指し、呼ぶ側がその参照を
    /// 1つ持つ。`handle` の型のハンドルでなければ、どちらのモードでも `WrongKind` を返す。
    pub unsafe fn handle_close(&mut self, h: Word, handle: Handle) -> (r: Result<(), Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(h),
            !M::CHECKED ==> old(self).holds(h),
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
            old(self).held_ok() && old(self).holds(h) ==> (r matches Err(Fault::Heap(e))
                ==> *final(self) == *old(self) || chain_err(Err::<(), HeapError>(e))),
            old(self).held_ok() && old(self).holds(h) && r is Ok ==> {
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
        let kind = match handle {
            Handle::FileIn => match unsafe { self.heap.foreign::<FileIn>(h) } {
                Ok(_) => Ok(()),
                Err(e) => Err(e),
            },
            Handle::FileOut => match unsafe { self.heap.foreign::<FileOut>(h) } {
                Ok(_) => Ok(()),
                Err(e) => Err(e),
            },
            Handle::Stdin => match unsafe { self.heap.foreign::<StdinHandle>(h) } {
                Ok(_) => Ok(()),
                Err(e) => Err(e),
            },
            Handle::Stdout => match unsafe { self.heap.foreign::<StdoutHandle>(h) } {
                Ok(_) => Ok(()),
                Err(e) => Err(e),
            },
            Handle::Stderr => match unsafe { self.heap.foreign::<StderrHandle>(h) } {
                Ok(_) => Ok(()),
                Err(e) => Err(e),
            },
        };
        if let Err(e) = kind {
            return Err(Fault::Heap(e));
        }
        unsafe { self.release_handle(h) }
    }

    /// 型を確かめたハンドル `h` の参照を手放す。`handle_close` と、書けなかった `handle_write` と `handle_flush` が使う。
    ///
    /// # Safety
    /// `handle_close` と同じである。
    unsafe fn release_handle(&mut self, h: Word) -> (r: Result<(), Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(h),
            h.is_ptr_spec(),
            !M::CHECKED ==> old(self).holds(h),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || (r matches Err(Fault::Heap(_))),
            r is Err ==> *final(self) == *old(self) || final(self).heap_spec().kept_live(
                old(self).heap_spec(),
            ),
            !M::CHECKED ==> r is Ok,
            old(self).held_ok() && old(self).holds(h) ==> (r matches Err(Fault::Heap(e))
                ==> *final(self) == *old(self) || chain_err(Err::<(), HeapError>(e))),
            old(self).held_ok() && old(self).holds(h) && r is Ok ==> {
                &&& final(self).held_ok()
                &&& final(self).evolved_by(*old(self), |x: usize| -points(h, x), Set::empty())
            },
    {
        let ghost s0 = *self;
        let ghost c = s0.held_ok() && s0.holds(h);
        proof {
            if c {
                s0.lemma_held_arg(h);
            }
        }
        if M::CHECKED {
            let header = self.heap.header_of(h);
            if let Err(e) = self.heap.checked_decref(header) {
                return Err(Fault::Heap(e));
            }
        }
        if let Err(e) = unsafe { self.heap.decref(h) } {
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
                self.lemma_closed(s0, h);
            }
        }
        Ok(())
    }

    /// `FileOut.write`、`write_bytes`、`write_line`、`Stdout.write`、`write_string`、`write_line` と、`Stderr` の同じ3つ。
    /// `data` を借りて `handle` のハンドル `h` に書き、`h` をそのまま返す。`write` がデータの種類と書き方を決め、`Line`
    /// は `println` と同じく `\n` を足して1行として書く。標準のストリームのハンドルは buffer を持たず Host の buffer に、
    /// `FileOut` は自分の buffer に書く (docs/spec/object-model.md の「出力のバッファ」)。書けなければ `h` を手放して、
    /// 標準のストリームなら `Output`、`FileOut` なら `Io { op: Write, .. }` を返す。`data` の参照の数は変わらない。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。`h` と `data` は即値か、このヒープか付けた塊の生きている物体を指し、呼ぶ側が
    /// それぞれの参照を1つ持つ。`handle` が `FileIn` か `Stdin` か、`h` が `handle` の型のハンドルでなければ、どちらの
    /// モードでも `WrongKind` を返す。速いモードでは、`data` は `write` の種類 (`Bytes` ならバイト列、ほかは文字列)
    /// で、`FileOut` の `h` は局所で rc が 1 である (検査モードは確かめ、違えば何も書かずに `WrongKind`、`NotUnique`、
    /// `WrongOwner` を返す)。
    pub unsafe fn handle_write(&mut self, h: Word, handle: Handle, data: Word, write: Write) -> (r:
        Result<Word, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(h),
            old(self).heap_spec().word_in(data),
            !M::CHECKED ==> old(self).is_body(
                data,
                if write == Write::Bytes {
                    Kind::Bytes
                } else {
                    Kind::String
                },
            ) && old(self).holds(data) && old(self).holds(h),
            !M::CHECKED && handle == Handle::FileOut ==> old(self).heap_spec().rc_value(h) == 1,
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || (r matches Err(Fault::Output(_))) || (r matches Err(
                Fault::Io { op: IoOp::Write, .. },
            )) || (r matches Err(Fault::Heap(_))),
            r matches Ok(x) ==> x == h,
            !M::CHECKED && (r matches Err(Fault::Heap(_))) ==> r == Err::<Word, Fault>(
                Fault::Heap(HeapError::WrongKind),
            ),
            old(self).held_ok() && old(self).is_body(
                data,
                if write == Write::Bytes {
                    Kind::Bytes
                } else {
                    Kind::String
                },
            ) && old(self).holds(data) && old(self).holds(h) ==> {
                &&& r is Ok ==> {
                    &&& final(self).held_ok()
                    &&& final(self).holds(h)
                    &&& final(self).evolved_by(*old(self), |x: usize| 0int, Set::empty())
                }
                &&& r is Err && !(r matches Err(Fault::Heap(_))) ==> {
                    &&& final(self).held_ok()
                    &&& final(self).evolved_by(*old(self), |x: usize| -points(h, x), Set::empty())
                }
            },
    {
        let ghost s0 = *self;
        let kind = match write {
            Write::Bytes => Kind::Bytes,
            Write::Text | Write::Line => Kind::String,
        };
        let ghost c = s0.held_ok() && s0.is_body(data, kind) && s0.holds(data) && s0.holds(h);
        if !h.is_ptr() || M::CHECKED && !data.is_ptr() {
            return Err(Fault::Heap(HeapError::WrongKind));
        }
        proof {
            if c {
                s0.lemma_held_arg(h);
            }
        }
        // 確かめはどれも書く前に済ませ、誤りなら何も書かない
        let checked = match handle {
            Handle::Stdout => match unsafe { self.heap.foreign::<StdoutHandle>(h) } {
                Ok(_) => Ok(Sink::Stream(Stream::Out)),
                Err(e) => Err(e),
            },
            Handle::Stderr => match unsafe { self.heap.foreign::<StderrHandle>(h) } {
                Ok(_) => Ok(Sink::Stream(Stream::Err)),
                Err(e) => Err(e),
            },
            Handle::FileOut => match unsafe { self.heap.foreign_unique::<FileOut>(h) } {
                Ok(p) => Ok(Sink::File(p)),
                Err(e) => Err(e),
            },
            // 読むためのハンドルには書けない
            Handle::FileIn | Handle::Stdin => Err(HeapError::WrongKind),
        };
        let sink = match checked {
            Ok(sink) => sink,
            Err(e) => {
                return Err(Fault::Heap(e));
            },
        };
        self.heap.settle();
        let ghost h1 = self.heap;
        let hd = self.heap.header_of(data);
        proof {
            lemma_settled_twice(s0.heap);
            assert(h1.words_kept(s0.heap));
            h1.lemma_text_spec(data);
            s0.heap.lemma_text_spec(data);
            if c {
                s0.lemma_settled_body(h1, data, kind);
            }
        }
        if let Err(e) = self.heap.checked_body(hd, kind) {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        let line = match write {
            Write::Line => true,
            Write::Bytes | Write::Text => false,
        };
        let written = match sink {
            Sink::Stream(stream) => {
                let to_host = if line {
                    host_write_line(&self.host, stream, self.heap.string_text(data))
                } else {
                    host_write(&self.host, stream, self.heap.body_of(data))
                };
                match to_host {
                    Ok(()) => Ok(()),
                    Err(e) => Err(Fault::Output(e)),
                }
            },
            Sink::File(p) => unsafe { file_out_write(p, self.heap.body_of(data), line) },
        };
        proof {
            if M::CHECKED {
                self.lemma_words_kept(s0);
            }
            if c {
                self.lemma_lent(s0, h1);
                self.lemma_still_held(s0, h);
            }
        }
        match written {
            Ok(()) => Ok(h),
            Err(fault) => {
                let released = unsafe { self.release_handle(h) };
                released?;
                proof {
                    if c {
                        lemma_settle_moved(h1, s0.heap);
                        lemma_moved_chain(
                            self.heap,
                            h1,
                            s0.heap,
                            |x: usize| 0int,
                            |x: usize| -points(h, x),
                            |x: usize| -points(h, x),
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

    /// `Stdout.flush` と `FileOut.flush`。標準出力なら Host がまとめて持っている出力を、`FileOut` なら自分の buffer を
    /// 書き出させ、受け取ったハンドル `h` をそのまま返す。書き出せなければ `h` を手放して、標準出力なら `Output`、
    /// `FileOut` なら `Io { op: Write, .. }` を返す。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。`h` は即値か、このヒープか付けた塊の生きている物体を指し、呼ぶ側がその参照を
    /// 1つ持つ。`handle` が `Stdout` か `FileOut` でないか、`h` が `handle` の型のハンドルでなければ、どちらのモードでも
    /// `WrongKind` を返す。速いモードでは、`FileOut` の `h` は局所で rc が 1 である (検査モードは確かめる)。
    pub unsafe fn handle_flush(&mut self, h: Word, handle: Handle) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(h),
            !M::CHECKED ==> old(self).holds(h),
            !M::CHECKED && handle == Handle::FileOut ==> old(self).heap_spec().rc_value(h) == 1,
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || (r matches Err(Fault::Output(_))) || (r matches Err(
                Fault::Io { op: IoOp::Write, .. },
            )) || (r matches Err(Fault::Heap(_))),
            r matches Ok(x) ==> x == h && *final(self) == *old(self),
            !M::CHECKED && (r matches Err(Fault::Heap(_))) ==> r == Err::<Word, Fault>(
                Fault::Heap(HeapError::WrongKind),
            ),
            old(self).held_ok() && old(self).holds(h) && r is Err && !(r matches Err(
                Fault::Heap(_),
            )) ==> final(self).held_ok() && final(self).evolved_by(
                *old(self),
                |x: usize| -points(h, x),
                Set::empty(),
            ),
    {
        let ghost s0 = *self;
        let ghost c = s0.held_ok() && s0.holds(h);
        if !h.is_ptr() {
            return Err(Fault::Heap(HeapError::WrongKind));
        }
        proof {
            if c {
                s0.lemma_held_arg(h);
            }
        }
        let flushed = match handle {
            Handle::Stdout => match unsafe { self.heap.foreign::<StdoutHandle>(h) } {
                Ok(_) => match host_flush(&self.host) {
                    Ok(()) => Ok(()),
                    Err(e) => Err(Fault::Output(e)),
                },
                Err(e) => {
                    return Err(Fault::Heap(e));
                },
            },
            Handle::FileOut => match unsafe { self.heap.foreign_unique::<FileOut>(h) } {
                Ok(p) => unsafe { file_out_flush(p) },
                Err(e) => {
                    return Err(Fault::Heap(e));
                },
            },
            Handle::FileIn | Handle::Stdin | Handle::Stderr => {
                return Err(Fault::Heap(HeapError::WrongKind));
            },
        };
        match flushed {
            Ok(()) => Ok(h),
            Err(fault) => {
                let released = unsafe { self.release_handle(h) };
                released?;
                Err(fault)
            },
        }
    }

    /// 読み手の補充の前に書き出させる Host。`flush` でなければ `None` である。
    fn flushed_host(&self, flush: bool) -> Option<&HostRef> {
        if flush {
            Some(&self.host)
        } else {
            None
        }
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

    /// 呼ぶ前の文脈 `s0` から何も手放していなければ、呼ぶ側が持っていた `w` は持たれたままである。
    proof fn lemma_still_held(&self, s0: Ctx<M>, w: Word)
        requires
            s0.held_ok(),
            s0.holds(w),
            moved(self.heap, s0.heap, |x: usize| 0int, Set::empty()),
            self.evv == s0.evv,
            self.yielding == s0.yielding,
        ensures
            self.holds(w),
    {
        let z = |x: usize| 0int;
        let ws = seq![w];
        assert(held_word(s0.heap, ws[0]));
        assert forall|x: usize| #[trigger] self.need(ws, x) > 0 implies s0.need(ws, x) + z(x)
            >= self.need(ws, x) by {
            lemma_points_in(ws, x);
        }
        self.lemma_holds_moved(s0, ws, ws, z, Set::empty());
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

    /// `FileIn.open` で FileIn を作った後も、パス `s` は持たれていて手放せる。`self` は呼ぶ前の文脈である。
    pub(super) proof fn lemma_path_kept(&self, h1: Heap<M>, h2: Heap<M>, s: Word, w: Word)
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
        self.lemma_settled_body(h1, s, Kind::String);
        lemma_alloc_moved(h2, h1, w);
        lemma_arg_moved(h2, h1, |x: usize| 0int, Set::empty(), s);
        lemma_view_live(h2, s.bits_spec());
    }

    /// `FileIn.open` の、開けた道でモデルが合っている道。`h1` は `settle` の後、`h2` は FileIn を作った後である。
    pub(super) proof fn lemma_opened(&self, s0: Ctx<M>, h1: Heap<M>, h2: Heap<M>, s: Word, w: Word)
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
        s0.lemma_settled_body(h1, s, Kind::String);
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

    /// 行の読みの途中で、呼ぶ前の文脈 `s0` から `moved` で変えただけなら、`Ctx` の語は持たれている。
    proof fn lemma_line_ok(&self, s0: Ctx<M>)
        requires
            s0.held_ok(),
            self.heap.consistent(),
            self.heap.settleable(),
            moved(self.heap, s0.heap, |x: usize| 0int, Set::empty()),
            self.evv == s0.evv,
            self.yielding == s0.yielding,
            self.max_evv == s0.max_evv,
        ensures
            self.held_ok(),
            self.evolved_by(s0, |x: usize| 0int, Set::empty()),
    {
        let z = |x: usize| 0int;
        let none = Set::<usize>::empty();
        assert forall|x: usize| #[trigger] self.need(seq![], x) > 0 implies s0.need(seq![], x) + z(
            x,
        ) >= self.need(seq![], x) by {
            lemma_points_in(seq![], x);
        }
        self.lemma_holds_moved(s0, seq![], seq![], z, none);
        self.lemma_evv_kept(s0, z, none);
    }

    /// 読み終えた行 `w` を返す道。呼ぶ側は `w` の参照を `Ctx` と別に1つ持つ。
    proof fn lemma_line_done(&self, s0: Ctx<M>, w: Word)
        requires
            s0.held_ok(),
            line_held(self.heap, s0.heap, w),
            self.evv == s0.evv,
            self.yielding == s0.yielding,
            self.max_evv == s0.max_evv,
        ensures
            self.held_ok(),
            self.holds(w),
            self.evolved_by(s0, |x: usize| 0int, Set::empty()),
    {
        self.lemma_line_ok(s0);
        lemma_view_live(self.heap, w.bits_spec());
        self.lemma_fresh_holds(s0, w);
    }

    /// 読みの誤りで、途中まで読んだ行 `w` を手放した道。`h1` は手放す前のヒープである。
    proof fn lemma_line_dropped(&self, s0: Ctx<M>, h1: Heap<M>, w: Word)
        requires
            s0.held_ok(),
            line_held(h1, s0.heap, w),
            self.heap.consistent(),
            self.heap.settled(),
            self.heap.evolved(h1, w, -1),
            self.heap.kept_live(h1),
            self.evv == s0.evv,
            self.yielding == s0.yielding,
            self.max_evv == s0.max_evv,
        ensures
            self.held_ok(),
            self.evolved_by(s0, |x: usize| 0int, Set::empty()),
    {
        let z = |x: usize| 0int;
        let dw = |x: usize| -points(w, x);
        let none = Set::<usize>::empty();
        lemma_decref_moved(self.heap, h1, w);
        assert forall|b: usize| live(s0.heap, b) implies #[trigger] z(b) == z(b) + dw(b) by {
            assert(b != w.bits_spec());
        }
        lemma_moved_chain(self.heap, h1, s0.heap, z, dw, z, none, none, none);
        self.lemma_line_ok(s0);
    }

    /// `handle_read` の、読めた道でモデルが合っている道。読んだ結果 `s` を作っただけで、借りたハンドルを含め、呼ぶ前から
    /// 生きている物体の参照の数は変わらない。
    proof fn lemma_read_lent(&self, s0: Ctx<M>, s: Word)
        requires
            s0.held_ok(),
            self.heap.made(s0.heap, s),
            self.heap.owned(s),
            self.evv == s0.evv,
            self.yielding == s0.yielding,
            self.max_evv == s0.max_evv,
        ensures
            self.held_ok(),
            self.holds(s),
            self.evolved_by(s0, |x: usize| 0int, Set::empty()),
    {
        let z = |x: usize| 0int;
        let none = Set::<usize>::empty();
        lemma_alloc_moved(self.heap, s0.heap, s);
        assert forall|x: usize| #[trigger] self.need(seq![], x) > 0 implies s0.need(seq![], x) + z(
            x,
        ) >= self.need(seq![], x) by {
            lemma_points_in(seq![], x);
        }
        self.lemma_holds_moved(s0, seq![], seq![], z, none);
        self.lemma_evv_kept(s0, z, none);
        lemma_view_live(self.heap, s.bits_spec());
        self.lemma_fresh_holds(s0, s);
    }

    /// `println` と、読みが失敗した塊の読みの、モデルが合っている道。`settle` の後の `h1` から `s` を手放した。
    pub(super) proof fn lemma_printed(&self, s0: Ctx<M>, h1: Heap<M>, s: Word)
        requires
            s0.held_ok(),
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
