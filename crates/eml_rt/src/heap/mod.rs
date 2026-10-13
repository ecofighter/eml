//! ヒープ: 確保、RC、解放の連鎖、フィールドの読み書き
//! (docs/spec/object-model.md の「核の操作」)。語を受ける操作と、ペイロードを
//! 呼ぶ側が書く `alloc` と `alloc_immortal` は `unsafe fn` で、安全の条件は docs/spec/object-model.md の「核の操作」にある。その条件の
//! 「生きている物体」は、`meta` に合うペイロード (走査する語、`scan` が 255 なら数の語、`BigInt` なら値) をすべて書いた
//! 物体を指す。データの生の語はこの条件に入らない。検査モードは、確かめられる違反を `Err` で返す。各メソッドの
//! `# Safety` には、速いモードが確かめず検査モードが `Err` にする条件も書く。

pub(crate) mod check;
mod release;
mod reuse;
pub(crate) mod share;
mod walk;

#[cfg(test)]
mod tests;

use std::cell::Cell;
use std::marker::PhantomData;
use std::mem;
use std::ptr::NonNull;

use crate::alloc::Pages;
use crate::error::HeapError;
use crate::foreign;
use crate::meta::{self, HEADER_WORDS, Header, Kind, Meta};
use crate::mode::Mode;
use crate::rc::{self, DecrefStep, DupStep};
use crate::word::Word;
use std::sync::atomic::Ordering;

pub use reuse::Token;

use check::{PREFIX_WORDS, State};

/// 検査モードだけで数える仕事の回数 (docs/spec/object-model.md の「モードの選び方」)。ダミーの1つも数に入る。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Stats {
    pub rc_increments: u64,
    pub rc_decrements: u64,
    pub allocations: u64,
    /// 生きている箱 (token の箱を含む) の数。
    pub live: u64,
    pub peak_live: u64,
}

pub struct Heap<M: Mode> {
    pages: Pages,
    worker: u32,
    /// 解放の連鎖の作業リスト。使い回して、解放のたびに確保しない。
    work: Vec<Word>,
    /// `share` のたどりのスタック。使い回す。
    share_stack: Vec<NonNull<Header>>,
    /// 検査モードの `share` が訪れた `lin` の物体。
    visited_lin: Vec<NonNull<Header>>,
    /// 検査モードの token の表。項目は箱と、`into_token` のときの世代の番号である。
    tokens: Vec<Option<(NonNull<Header>, u32)>>,
    dummy: Word,
    /// 検査モードで隔離した (使い回さない) ブロックのバイト数。
    quarantined: usize,
    stats: Stats,
    _mode: PhantomData<(M, Cell<()>)>,
}

// `Heap` は worker ごとに1つ持ち、worker のスレッドに移せる。`Sync` にはしない (docs/spec/object-model.md の「語」)
unsafe impl<M: Mode> Send for Heap<M> {}

impl<M: Mode> Default for Heap<M> {
    fn default() -> Heap<M> {
        Heap::new()
    }
}

/// ペイロードの語の数から、ヘッダを含む物体の語の数を出す。空いたブロックのペイロードの先頭に空きリストのポインタを
/// 置くので、ペイロードは少なくとも1語にする。
#[inline]
fn object_words(payload_words: usize) -> usize {
    // ふだんのビルドで桁あふれが回ると小さいブロックを取り、安全な `alloc_data` がその先まで書いてしまう
    // (docs/spec/object-model.md の「ページ」)
    HEADER_WORDS
        .checked_add(payload_words.max(1))
        .expect(TOO_LARGE)
}

/// 語の数の計算が桁あふれしたときの panic の文言。
pub(crate) const TOO_LARGE: &str = "an object of more words than the address space holds";

/// 走査する語の数。`scan` が 255 なら、走査の始まりの生の語が数を持つ。
#[inline]
unsafe fn scan_count(h: NonNull<Header>) -> usize {
    let m = unsafe { meta::meta(h) };
    if m.scan() == Meta::EXTENDED_SCAN {
        unsafe { meta::slot(h, m.kind().scan_start()).cast::<u64>().read() as usize }
    } else {
        usize::from(m.scan())
    }
}

/// 走査する語の i 番目の位置。
#[inline]
unsafe fn scan_slot(h: NonNull<Header>, i: usize) -> *mut Word {
    let m = unsafe { meta::meta(h) };
    let start = m.kind().scan_start() + usize::from(m.scan() == Meta::EXTENDED_SCAN);
    unsafe { meta::slot(h, start + i) }
}

impl<M: Mode> Heap<M> {
    pub fn new() -> Heap<M> {
        let mut heap = Heap {
            pages: Pages::new(if M::CHECKED { PREFIX_WORDS } else { 0 }),
            worker: 0,
            work: Vec::new(),
            share_stack: Vec::new(),
            visited_lin: Vec::new(),
            tokens: Vec::new(),
            dummy: Word::EMPTY,
            quarantined: 0,
            stats: Stats::default(),
            _mode: PhantomData,
        };
        // ダミーは値 0 の BigInt の不死の物体で、検査モードでも数えない (docs/spec/object-model.md の「ダミーの値」)
        let h = heap.alloc_header(1, 0, Meta::new(0, 0, Kind::BigInt, false));
        unsafe { meta::slot(h, 0).cast::<i64>().write(0) };
        if M::CHECKED {
            unsafe { check::prefix(h) }
                .first
                .store(check::UNCOUNTED, Ordering::Relaxed);
        }
        heap.dummy = Word::from_header(h);
        heap
    }

    pub fn dummy(&self) -> Word {
        self.dummy
    }

    pub fn worker(&self) -> u32 {
        self.worker
    }

    /// 論理的な worker ID を替える。これから確保する物体の所有者と、所有者の検査の相手になる。
    pub fn set_worker(&mut self, worker: u32) {
        self.worker = worker;
    }

    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// 使っているメモリのバイト数 (docs/spec/object-model.md の「確保と解放」)。検査モードでは隔離したブロックを引く。
    pub fn bytes_in_use(&self) -> usize {
        self.pages.bytes() - self.quarantined
    }

    /// ブロックを取り、ヘッダと (検査モードでは) 前置きを書く。ペイロードは書かない。
    #[inline]
    fn alloc_header(&mut self, payload_words: usize, rc: i32, meta: Meta) -> NonNull<Header> {
        debug_assert_ne!(meta.kind(), Kind::Free);
        let h = self.pages.alloc(object_words(payload_words));
        unsafe { meta::init(h, rc, meta) };
        if M::CHECKED {
            unsafe { check::init_prefix(h, if rc == 0 { 0 } else { self.worker }) };
            self.stats.allocations += 1;
            self.stats.live += 1;
            self.stats.peak_live = self.stats.peak_live.max(self.stats.live);
        }
        h
    }

    /// 物体を確保する。ペイロードは呼ぶ側が書く。
    ///
    /// # Safety
    /// `meta` の種類は `Foreign` でも `Free` でもなく、呼ぶ側は、ほかのヒープの操作の前に、`meta` に合うペイロード
    /// (走査する語、`scan` が 255 なら数の語、`BigInt` なら値) を書く。
    #[inline]
    pub unsafe fn alloc(&mut self, payload_words: usize, meta: Meta) -> Word {
        debug_assert_ne!(meta.kind(), Kind::Foreign);
        Word::from_header(self.alloc_header(payload_words, 1, meta))
    }

    /// 不死の物体を確保する。検査モードでは、外に出ている参照の数を 0 にして作る。
    ///
    /// # Safety
    /// `alloc` と同じ。
    pub unsafe fn alloc_immortal(&mut self, payload_words: usize, meta: Meta) -> Word {
        debug_assert_ne!(meta.kind(), Kind::Foreign);
        Word::from_header(self.alloc_header(payload_words, 0, meta))
    }

    /// データの物体を作る。走査する語が 255 個以上なら、ペイロードの先頭に数の語を置く。走査する語は、書く前に
    /// 解放の連鎖や `share` が読んでも子をたどらないように、`()` で埋めておく。
    pub fn alloc_data(&mut self, tag: u16, scanned: usize, raw: usize, lin: bool) -> Word {
        let extended = scanned >= usize::from(Meta::EXTENDED_SCAN);
        let scan = if extended {
            Meta::EXTENDED_SCAN
        } else {
            scanned as u8
        };
        let payload_words = usize::from(extended)
            .checked_add(scanned)
            .and_then(|words| words.checked_add(raw))
            .expect(TOO_LARGE);
        let h = self.alloc_header(payload_words, 1, Meta::new(tag, scan, Kind::Data, lin));
        unsafe {
            if extended {
                meta::slot(h, 0).cast::<u64>().write(scanned as u64);
            }
            for i in 0..scanned {
                scan_slot(h, i).write(Word::UNIT);
            }
        }
        Word::from_header(h)
    }

    /// `Int` を `tobj` に入れる。63 ビットに収まれば即値、収まらなければ `BigInt` の物体にする。
    pub fn alloc_int(&mut self, n: i64) -> Word {
        Word::int(n).unwrap_or_else(|| {
            let h = self.alloc_header(1, 1, Meta::new(0, 0, Kind::BigInt, false));
            unsafe { meta::slot(h, 0).cast::<i64>().write(n) };
            Word::from_header(h)
        })
    }

    pub fn alloc_foreign<T: Send + 'static>(&mut self, value: T, lin: bool) -> Word {
        const {
            assert!(
                align_of::<T>() <= 8,
                "a foreign value must be at most word-aligned"
            )
        };
        let h = self.alloc_header(
            1 + size_of::<T>().div_ceil(8),
            1,
            Meta::new(0, 0, Kind::Foreign, lin),
        );
        unsafe { foreign::write(h, value) };
        Word::from_header(h)
    }

    /// 不死の物体の参照を1つ作る (docs/spec/object-model.md の「不死の物体の数え方」)。速いモードでは何もしない。
    ///
    /// # Safety
    /// `w` は即値か、このヒープの生きている不死の物体 (rc 0) を指す。
    pub unsafe fn acquire_immortal(&mut self, w: Word) -> Result<(), HeapError> {
        if M::CHECKED && w.is_ptr() {
            let h = w.header();
            unsafe { self.check_live(h)? };
            if unsafe { meta::rc(h) }.load(Ordering::Relaxed) != 0 {
                return Err(HeapError::NotImmortal);
            }
            if unsafe { check::count_immortal(h, true)? } {
                self.stats.rc_increments += 1;
            }
        }
        Ok(())
    }

    /// # Safety
    /// `w` は即値か、このヒープの生きている物体を指し、呼ぶ側が参照を持っているか、生きている持ち主から借りている。
    /// 物体は `lin` ビットを持たない。rc が正なら、持ち主は今の worker である。
    #[inline]
    pub unsafe fn dup(&mut self, w: Word) -> Result<(), HeapError> {
        if !w.is_ptr() {
            return Ok(());
        }
        let h = w.header();
        // 局所の速い道だけを呼ぶ側に展開し、`Result` の分岐を消す (docs/spec/object-model.md の「核の操作」)。検査モードは
        // rc より先に前置きを読むので、いつも遅い道に進む
        if !M::CHECKED {
            let rc = unsafe { meta::rc(h) };
            if let DupStep::Store(n) = rc::dup_step(rc.load(Ordering::Relaxed)) {
                rc.store(n, Ordering::Relaxed);
                return Ok(());
            }
        }
        unsafe { self.dup_slow(h) }
    }

    /// `dup` の遅い道。共有、不死、張り付いた数と、検査モードの仕事を受け持つ。
    #[cold]
    #[inline(never)]
    unsafe fn dup_slow(&mut self, h: NonNull<Header>) -> Result<(), HeapError> {
        let counted = unsafe { self.check_dup(h)? };
        let rc = unsafe { meta::rc(h) };
        match rc::dup_step(rc.load(Ordering::Relaxed)) {
            DupStep::Store(n) => rc.store(n, Ordering::Relaxed),
            DupStep::Shared => rc::dup_shared(rc),
            DupStep::Skip => {}
        }
        if counted {
            self.stats.rc_increments += 1;
        }
        Ok(())
    }

    /// 参照を1つ手放す。LIR の `decref` と `drop` は、どちらもこれで実行する (docs/spec/object-model.md の「核の操作」)。
    ///
    /// # Safety
    /// `w` は即値か、このヒープの生きている物体を指し、呼ぶ側がその参照を1つ持っている。不死の物体なら、その参照は
    /// `acquire_immortal` か `dup` で作ったものである。共有の物体は `lin` ビットを持たない。rc が正なら、持ち主は今の worker である。
    #[inline]
    pub unsafe fn decref(&mut self, w: Word) -> Result<(), HeapError> {
        if !w.is_ptr() {
            return Ok(());
        }
        let h = w.header();
        // `dup` と同じく、局所の数を減らすだけの道を呼ぶ側に展開する
        if !M::CHECKED {
            let rc = unsafe { meta::rc(h) };
            if let DecrefStep::Store(n) = rc::decref_step(rc.load(Ordering::Relaxed)) {
                rc.store(n, Ordering::Relaxed);
                return Ok(());
            }
        }
        unsafe { self.decref_slow(h) }
    }

    /// `decref` の遅い道。解放の連鎖、共有、不死、張り付いた数と、検査モードの仕事を受け持つ。
    #[cold]
    #[inline(never)]
    unsafe fn decref_slow(&mut self, h: NonNull<Header>) -> Result<(), HeapError> {
        if unsafe { self.release_ref(h)? } {
            unsafe { self.free_chain(h)? };
        }
        Ok(())
    }

    /// 参照を1つ手放し、最後の参照なら真を返す。箱はまだ空けない。
    unsafe fn release_ref(&mut self, h: NonNull<Header>) -> Result<bool, HeapError> {
        let counted = unsafe { self.check_decref(h)? };
        let rc = unsafe { meta::rc(h) };
        let last = match rc::decref_step(rc.load(Ordering::Relaxed)) {
            DecrefStep::Store(n) => {
                rc.store(n, Ordering::Relaxed);
                false
            }
            DecrefStep::Free => true,
            DecrefStep::Shared => rc::decref_shared(rc),
            DecrefStep::Skip => false,
        };
        if counted {
            self.stats.rc_decrements += 1;
        }
        Ok(last)
    }

    unsafe fn free_chain(&mut self, root: NonNull<Header>) -> Result<(), HeapError> {
        let mut work = mem::take(&mut self.work);
        work.push(Word::from_header(root));
        let result = unsafe { self.drain(&mut work) };
        work.clear();
        self.work = work;
        result
    }

    /// 最後の参照を失った物体を空け、それで最後の参照を失った子を作業リストに積む。子は走査の逆の順に手放す。最初の
    /// フィールドを先に、最後のフィールド (cons セルの残りのリスト) を後に空けて、作業リストを短く保つためである。
    unsafe fn drain(&mut self, work: &mut Vec<Word>) -> Result<(), HeapError> {
        while let Some(w) = work.pop() {
            let h = w.header();
            for i in (0..unsafe { scan_count(h) }).rev() {
                let child = unsafe { scan_slot(h, i).read() };
                if child.is_ptr() && unsafe { self.release_ref(child.header())? } {
                    work.push(child);
                }
            }
            unsafe { self.free_box(h) };
        }
        Ok(())
    }

    /// 箱を空ける。`Foreign` なら破棄処理を呼ぶ。破棄処理が panic しても同じ値を二度破棄しないように、箱を先に
    /// 死んだことにしてから呼ぶ (箱は漏れてよい)。検査モードは箱を使い回さず、前置きを「解放」にして隔離する。
    #[inline]
    unsafe fn free_box(&mut self, h: NonNull<Header>) {
        let finalizer = unsafe { meta::meta(h) }
            .kind()
            .has_finalizer()
            .then(|| unsafe { foreign::finalizer(h) });
        if M::CHECKED {
            unsafe { check::prefix(h) }.set_state(State::Freed, true);
            self.quarantined += unsafe { self.pages.block_bytes(h) };
            self.stats.live -= 1;
        } else if finalizer.is_some() {
            unsafe { meta::set_meta(h, Meta::new(0, 0, Kind::Free, false)) };
        }
        if let Some(finalizer) = finalizer {
            unsafe { finalizer.run() };
        }
        if !M::CHECKED {
            unsafe { self.pages.free(h) };
        }
    }

    /// rc が 1 か。reuse とその場の書き換えに使う。共有、不死、張り付いた数は、この比較でみな偽になる。
    ///
    /// # Safety
    /// `w` は即値か、このヒープの生きている物体を指す。rc が正なら、持ち主は今の worker である。
    pub unsafe fn is_unique(&self, w: Word) -> Result<bool, HeapError> {
        if !w.is_ptr() {
            return Ok(false);
        }
        let h = w.header();
        unsafe { self.check_live(h)? };
        let rc = unsafe { meta::rc(h) }.load(Ordering::Relaxed);
        unsafe { self.check_owner(h, rc)? };
        Ok(rc == 1)
    }

    /// 手放そうとしている参照が最後のものか (rc が 1 か -1)。フィールドを `dup` せずに受け取ってよいかに使う。
    ///
    /// # Safety
    /// `w` は即値か、このヒープの生きている物体を指す。rc が正なら、持ち主は今の worker である。
    pub unsafe fn is_last(&self, w: Word) -> Result<bool, HeapError> {
        if !w.is_ptr() {
            return Ok(false);
        }
        let h = w.header();
        unsafe { self.check_live(h)? };
        let rc = unsafe { meta::rc(h) };
        let last = rc::is_last(rc);
        unsafe { self.check_owner(h, rc.load(Ordering::Relaxed))? };
        Ok(last)
    }

    /// `is_last` が真で、子をすでに処理した物体の箱を空ける。ヘッダの `scan` を読まない。
    ///
    /// # Safety
    /// `w` はこのヒープの生きている物体を指し、呼ぶ側が最後の参照を持ち (rc が 1 か -1)、子の参照はもう処理してある。
    /// 種類は破棄処理を持たない (`Foreign` でない)。rc が正なら、持ち主は今の worker である。
    pub unsafe fn free_block(&mut self, w: Word) -> Result<(), HeapError> {
        let h = w.header();
        if M::CHECKED {
            unsafe { self.check_live(h)? };
            if unsafe { meta::meta(h) }.kind().has_finalizer() {
                return Err(HeapError::FinalizerSkipped);
            }
            let rc = unsafe { meta::rc(h) };
            if !rc::is_last(rc) {
                return Err(HeapError::NotLast);
            }
            unsafe { self.check_owner(h, rc.load(Ordering::Relaxed))? };
            self.stats.rc_decrements += 1;
        }
        unsafe { self.free_box(h) };
        Ok(())
    }

    /// # Safety
    /// `w` はこのヒープの生きている物体を指す。
    pub unsafe fn meta(&self, w: Word) -> Result<Meta, HeapError> {
        unsafe { self.check_live(w.header())? };
        Ok(unsafe { meta::meta(w.header()) })
    }

    /// ペイロードの物理的な i 語目を読む。ヘッダを読まない。
    ///
    /// # Safety
    /// `w` はこのヒープの生きている物体を指し、i 語目はペイロードの中の走査する語である。
    pub unsafe fn field(&self, w: Word, i: usize) -> Result<Word, HeapError> {
        let h = w.header();
        unsafe {
            self.check_live(h)?;
            self.check_bounds(h, i)?;
            Ok(meta::slot(h, i).read())
        }
    }

    /// ペイロードの物理的な i 語目を書く。参照の数は変えない (所有の受け渡しは呼ぶ側が決める)。
    ///
    /// # Safety
    /// `w` はこのヒープの生きている物体を指し、i 語目はペイロードの中の走査する語で、ほかのスレッドはその物体を読めない。
    /// rc が正なら、持ち主は今の worker である。
    pub unsafe fn set_field(&mut self, w: Word, i: usize, v: Word) -> Result<(), HeapError> {
        let h = w.header();
        unsafe {
            self.check_live(h)?;
            self.check_bounds(h, i)?;
            self.check_owner(h, meta::rc(h).load(Ordering::Relaxed))?;
            meta::slot(h, i).write(v);
        }
        Ok(())
    }

    /// # Safety
    /// `w` はこのヒープの生きている物体を指し、i 語目はペイロードの中の生の語で、すでに書いてある。`alloc_data` は生の
    /// 語を書かず、新しいページのメモリは初期化されていないので、書く前に読むと未初期化の読み込みになる。この条件は
    /// 検査モードも確かめない。
    pub unsafe fn raw(&self, w: Word, i: usize) -> Result<u64, HeapError> {
        let h = w.header();
        unsafe {
            self.check_live(h)?;
            self.check_bounds(h, i)?;
            Ok(meta::slot(h, i).cast::<u64>().read())
        }
    }

    /// # Safety
    /// `w` はこのヒープの生きている物体を指し、i 語目はペイロードの中の生の語で、ほかのスレッドはその物体を読めない。
    /// rc が正なら、持ち主は今の worker である。
    pub unsafe fn set_raw(&mut self, w: Word, i: usize, v: u64) -> Result<(), HeapError> {
        let h = w.header();
        unsafe {
            self.check_live(h)?;
            self.check_bounds(h, i)?;
            self.check_owner(h, meta::rc(h).load(Ordering::Relaxed))?;
            meta::slot(h, i).cast::<u64>().write(v);
        }
        Ok(())
    }

    /// 走査する語の数。形を知らない処理が使う。
    ///
    /// # Safety
    /// `w` はこのヒープの生きている物体を指す。
    pub unsafe fn scanned_len(&self, w: Word) -> Result<usize, HeapError> {
        unsafe { self.check_live(w.header())? };
        Ok(unsafe { scan_count(w.header()) })
    }

    /// 走査する語の i 番目を読む。`scan` が 255 のときの数の語と、種類の走査の始まりを考える。
    ///
    /// # Safety
    /// `w` はこのヒープの生きている物体を指し、i は `scanned_len(w)` より小さい。
    pub unsafe fn scanned(&self, w: Word, i: usize) -> Result<Word, HeapError> {
        let h = w.header();
        unsafe { self.check_live(h)? };
        if M::CHECKED && i >= unsafe { scan_count(h) } {
            return Err(HeapError::OutOfBounds);
        }
        Ok(unsafe { scan_slot(h, i).read() })
    }

    /// `tobj` の `Int` を読む。即値なら右シフトし、物体なら `BigInt` の中身を読む。
    ///
    /// # Safety
    /// `w` は即値か、このヒープの生きている `BigInt` の物体 (ダミーを含む) を指す。
    pub unsafe fn unbox_int(&self, w: Word) -> Result<i64, HeapError> {
        if w.is_imm() {
            return Ok(w.imm_value());
        }
        let h = w.header();
        unsafe { self.check_live(h)? };
        if M::CHECKED && unsafe { meta::meta(h) }.kind() != Kind::BigInt {
            return Err(HeapError::WrongKind);
        }
        Ok(unsafe { meta::slot(h, 0).cast::<i64>().read() })
    }

    /// `Foreign` の物体の Rust の値の位置。物体が `Foreign` でないか、値が `T` でなければ `WrongKind` を返す。この照らし
    /// 合わせは、速いモードでも行う (docs/spec/object-model.md の「核の操作」)。
    ///
    /// # Safety
    /// `w` はこのヒープの生きている物体を指す。
    pub unsafe fn foreign<T: 'static>(&self, w: Word) -> Result<*mut T, HeapError> {
        let h = w.header();
        unsafe { self.check_live(h)? };
        if unsafe { meta::meta(h) }.kind() != Kind::Foreign {
            return Err(HeapError::WrongKind);
        }
        unsafe { foreign::value::<T>(h) }.ok_or(HeapError::WrongKind)
    }
}
