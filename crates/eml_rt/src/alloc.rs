//! ページ単位の確保器 (docs/spec/object-model.md の「ページ」と「確保と解放」)。物体は
//! 64 KiB に揃えたページから切り出し、大きさはヘッダでなくページの頭が持つ。ヘッダの中身はここでは読まない。

use std::num::NonZero;
use std::ptr::{self, NonNull};

use crate::heap::TOO_LARGE;
use crate::meta::{self, Header, Kind, Meta};
use std::alloc::{Layout, alloc, dealloc, handle_alloc_error};

pub(crate) const PAGE_BYTES: usize = 1 << 16;
const WORD: usize = 8;

/// 小さい物体の級のブロックの語の数 (ヘッダと、検査モードでは前置きを含む)。2 語から 16 語は1語刻みにする。
const CLASS_WORDS: [usize; 27] = [
    2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 20, 24, 28, 32, 40, 48, 56, 64, 80, 96,
    112, 128,
];
const CLASSES: usize = CLASS_WORDS.len();
const MAX_SMALL_WORDS: usize = 128;

/// ブロックの語の数から級を引く表。確保の速い道で級を探して回らないためである。
const CLASS_OF: [u8; MAX_SMALL_WORDS + 1] = {
    let mut table = [0; MAX_SMALL_WORDS + 1];
    let (mut words, mut class) = (0, 0);
    while words <= MAX_SMALL_WORDS {
        while CLASS_WORDS[class] < words {
            class += 1;
        }
        table[words] = class as u8;
        words += 1;
    }
    table
};

/// 大きい物体のページの級。
const LARGE: u32 = u32::MAX;

#[inline]
fn class_of(block_words: usize) -> Option<usize> {
    CLASS_OF.get(block_words).map(|&class| usize::from(class))
}

#[repr(C)]
struct PageHeader {
    class: u32,
    block_words: u32,
    /// `Pages::list` の中の添字。大きい物体のページを `swap_remove` で外すときに、移ってきたページの添字を直す。
    index: usize,
    /// 切り出した終わりの位置 (ページの先頭からのバイト数)。今のページではヒープの欄だけが正しく、ページを替える
    /// ときに書き戻す。
    limit: usize,
    /// `alloc` に渡した大きさ。
    bytes: usize,
}

const FIRST_BLOCK: usize = size_of::<PageHeader>().next_multiple_of(WORD);

#[derive(Clone, Copy)]
struct Class {
    /// 空いたブロックの LIFO の先頭。次のブロックは、空いたブロックのペイロードの先頭の語にある。
    free: *mut Header,
    page: *mut PageHeader,
    cursor: *mut u8,
    end: *mut u8,
}

impl Class {
    const EMPTY: Class = Class {
        free: ptr::null_mut(),
        page: ptr::null_mut(),
        cursor: ptr::null_mut(),
        end: ptr::null_mut(),
    };
}

pub(crate) struct Pages {
    prefix_words: usize,
    list: Vec<NonNull<PageHeader>>,
    classes: [Class; CLASSES],
    bytes: usize,
}

/// ブロックはどれもページの先頭から 64 KiB の内にあるので、下位 16 ビットを落とせばページの頭になる。
#[inline]
fn page_of(h: NonNull<Header>) -> NonNull<PageHeader> {
    h.cast::<u8>()
        .map_addr(|addr| {
            NonZero::new(addr.get() & !(PAGE_BYTES - 1)).expect("a page is not at address 0")
        })
        .cast()
}

impl Pages {
    pub(crate) fn new(prefix_words: usize) -> Pages {
        Pages {
            prefix_words,
            list: Vec::new(),
            classes: [Class::EMPTY; CLASSES],
            bytes: 0,
        }
    }

    /// OS から取ったページのバイト数。
    pub(crate) fn bytes(&self) -> usize {
        self.bytes
    }

    /// `words` 語 (ヘッダを含み、前置きを含まない) の物体のブロックを取り、ヘッダの位置を返す。ヘッダは書かない。
    #[inline]
    pub(crate) fn alloc(&mut self, words: usize) -> NonNull<Header> {
        let block_words = words.checked_add(self.prefix_words).expect(TOO_LARGE);
        match class_of(block_words) {
            Some(class) => self.alloc_small(class),
            None => self.alloc_large(block_words),
        }
    }

    #[inline]
    fn alloc_small(&mut self, class: usize) -> NonNull<Header> {
        if let Some(h) = NonNull::new(self.classes[class].free) {
            self.classes[class].free = unsafe { meta::slot(h, 0).cast::<*mut Header>().read() };
            return h;
        }
        let block_bytes = CLASS_WORDS[class] * WORD;
        let state = self.classes[class];
        if state.cursor.is_null() || state.end.addr() - state.cursor.addr() < block_bytes {
            self.next_page(class);
        }
        let state = &mut self.classes[class];
        let block = state.cursor;
        state.cursor = unsafe { block.add(block_bytes) };
        unsafe { NonNull::new_unchecked(block.add(self.prefix_words * WORD)).cast() }
    }

    #[cold]
    fn next_page(&mut self, class: usize) {
        let old = self.classes[class];
        if let Some(page) = NonNull::new(old.page) {
            unsafe { (*page.as_ptr()).limit = old.cursor.addr() - page.addr().get() };
        }
        let page = self.new_page(PAGE_BYTES, class as u32, CLASS_WORDS[class]);
        let base = page.as_ptr().cast::<u8>();
        let state = &mut self.classes[class];
        state.page = page.as_ptr();
        state.cursor = unsafe { base.add(FIRST_BLOCK) };
        state.end = unsafe { base.add(PAGE_BYTES) };
    }

    #[cold]
    fn alloc_large(&mut self, block_words: usize) -> NonNull<Header> {
        let bytes = block_words
            .checked_mul(WORD)
            .and_then(|bytes| bytes.checked_add(FIRST_BLOCK))
            .expect(TOO_LARGE);
        let page = self.new_page(bytes, LARGE, block_words);
        unsafe { (*page.as_ptr()).limit = bytes };
        let block = unsafe { page.as_ptr().cast::<u8>().add(FIRST_BLOCK) };
        unsafe { NonNull::new_unchecked(block.add(self.prefix_words * WORD)).cast() }
    }

    fn new_page(&mut self, bytes: usize, class: u32, block_words: usize) -> NonNull<PageHeader> {
        let layout = Layout::from_size_align(bytes, PAGE_BYTES).expect("a page layout");
        let Some(page) = NonNull::new(unsafe { alloc(layout) }.cast::<PageHeader>()) else {
            handle_alloc_error(layout)
        };
        let header = PageHeader {
            class,
            block_words: u32::try_from(block_words).expect("an object of fewer than 2^32 words"),
            index: self.list.len(),
            limit: FIRST_BLOCK,
            bytes,
        };
        unsafe { page.as_ptr().write(header) };
        self.list.push(page);
        self.bytes += bytes;
        page
    }

    /// 速いモードで物体の箱を空ける。小さい物体のブロックは種類 `Free` の印と次のブロックを書いて空きリストに積む。
    /// 印を書くのは、速いモードでもページを歩いて生きている物体を見分けるためである (docs/spec/object-model.md の「生存の列挙」)。
    /// 大きい物体のページはすぐ返す。
    ///
    /// # Safety
    /// `h` はこの `Pages` が確保した、まだ空けていないブロックのヘッダを指し、ペイロードは1語以上ある。
    #[inline]
    pub(crate) unsafe fn free(&mut self, h: NonNull<Header>) {
        let page = page_of(h);
        let class = unsafe { (*page.as_ptr()).class };
        if class == LARGE {
            unsafe { self.release_large(page) };
            return;
        }
        let state = &mut self.classes[class as usize];
        unsafe {
            meta::set_meta(h, Meta::new(0, 0, Kind::Free, false));
            meta::slot(h, 0).cast::<*mut Header>().write(state.free);
        }
        state.free = h.as_ptr();
    }

    #[cold]
    unsafe fn release_large(&mut self, page: NonNull<PageHeader>) {
        let (index, bytes) = unsafe { ((*page.as_ptr()).index, (*page.as_ptr()).bytes) };
        self.list.swap_remove(index);
        if let Some(moved) = self.list.get(index) {
            unsafe { (*moved.as_ptr()).index = index };
        }
        self.bytes -= bytes;
        unsafe {
            dealloc(
                page.as_ptr().cast(),
                Layout::from_size_align_unchecked(bytes, PAGE_BYTES),
            )
        };
    }

    /// ブロックの語の数 (前置きを含む)。
    ///
    /// # Safety
    /// `h` はこの `Pages` が確保したブロックのヘッダを指す。
    #[inline]
    pub(crate) unsafe fn block_words(&self, h: NonNull<Header>) -> usize {
        unsafe { (*page_of(h).as_ptr()).block_words as usize }
    }

    /// ブロックが使うバイト数。大きい物体ではページ全体である。検査モードの隔離の数え方に使う。
    ///
    /// # Safety
    /// `h` はこの `Pages` が確保したブロックのヘッダを指す。
    #[inline]
    pub(crate) unsafe fn block_bytes(&self, h: NonNull<Header>) -> usize {
        let page = page_of(h);
        unsafe {
            if (*page.as_ptr()).class == LARGE {
                (*page.as_ptr()).bytes
            } else {
                (*page.as_ptr()).block_words as usize * WORD
            }
        }
    }

    /// 箱が `words` 語の物体の級か (token の検査)。
    ///
    /// # Safety
    /// `h` はこの `Pages` が確保したブロックのヘッダを指す。
    #[inline]
    pub(crate) unsafe fn fits(&self, h: NonNull<Header>, words: usize) -> bool {
        let block_words = words.checked_add(self.prefix_words).expect(TOO_LARGE);
        let want = class_of(block_words).map_or(block_words, |class| CLASS_WORDS[class]);
        unsafe { self.block_words(h) == want }
    }

    /// 切り出したすべてのブロックのヘッダの位置を呼ぶ。空いたブロックも含むので、見分けるのは呼ぶ側である。各級の
    /// 今のページではヒープの切り出しの位置を読み、まだ切り出していないメモリをブロックとして読まない。
    pub(crate) fn for_each_block(&self, mut f: impl FnMut(NonNull<Header>)) {
        for &page in &self.list {
            let (class, block_words, limit) = unsafe {
                let header = &*page.as_ptr();
                (header.class, header.block_words as usize, header.limit)
            };
            let limit = match self.classes.get(class as usize) {
                Some(state) if state.page == page.as_ptr() => {
                    state.cursor.addr() - page.addr().get()
                }
                _ => limit,
            };
            let base = page.as_ptr().cast::<u8>();
            let block_bytes = block_words * WORD;
            let mut offset = FIRST_BLOCK;
            while offset + block_bytes <= limit {
                f(unsafe {
                    NonNull::new_unchecked(base.add(offset + self.prefix_words * WORD)).cast()
                });
                offset += block_bytes;
            }
        }
    }
}

impl Drop for Pages {
    fn drop(&mut self) {
        for page in self.list.drain(..) {
            unsafe {
                let bytes = (*page.as_ptr()).bytes;
                dealloc(
                    page.as_ptr().cast(),
                    Layout::from_size_align_unchecked(bytes, PAGE_BYTES),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init(h: NonNull<Header>) {
        unsafe { meta::init(h, 1, Meta::new(0, 0, Kind::Data, false)) }
    }

    fn blocks(pages: &Pages) -> usize {
        let mut count = 0;
        pages.for_each_block(|_| count += 1);
        count
    }

    #[test]
    fn the_class_table_rounds_up_to_the_next_class() {
        assert_eq!(class_of(0), Some(0));
        assert_eq!(class_of(2), Some(0));
        assert_eq!(class_of(3), Some(1));
        assert_eq!(class_of(16), Some(14));
        assert_eq!(CLASS_WORDS[class_of(17).unwrap()], 20);
        assert_eq!(class_of(128), Some(CLASSES - 1));
        assert_eq!(class_of(129), None);
    }

    #[test]
    fn blocks_of_one_class_are_carved_back_to_back() {
        let mut pages = Pages::new(0);
        let a = pages.alloc(3);
        let b = pages.alloc(3);
        assert_eq!(a.addr().get() % WORD, 0);
        assert_eq!(b.addr().get() - a.addr().get(), 3 * WORD);
        assert_eq!(unsafe { pages.block_words(a) }, 3);
        assert_eq!(pages.bytes(), PAGE_BYTES);
    }

    #[test]
    fn a_freed_block_is_marked_free_and_reused_first() {
        let mut pages = Pages::new(0);
        let a = pages.alloc(2);
        init(a);
        init(pages.alloc(2));
        unsafe { pages.free(a) };
        assert_eq!(unsafe { meta::meta(a) }.kind(), Kind::Free);
        assert_eq!(pages.alloc(2), a);
    }

    #[test]
    fn a_new_page_writes_back_where_the_old_one_stopped() {
        let mut pages = Pages::new(0);
        let per_page = (PAGE_BYTES - FIRST_BLOCK) / (16 * WORD);
        for _ in 0..per_page + 3 {
            init(pages.alloc(16));
        }
        assert_eq!(pages.list.len(), 2);
        assert_eq!(blocks(&pages), per_page + 3);
    }

    #[test]
    fn a_large_object_gets_its_own_page_and_is_returned_at_once() {
        let mut pages = Pages::new(0);
        init(pages.alloc(2));
        let [a, b, c] = [200, 300, 400].map(|words| {
            let h = pages.alloc(words);
            init(h);
            h
        });
        assert_eq!(pages.list.len(), 4);
        let before = pages.bytes();
        unsafe { pages.free(a) };
        assert_eq!(pages.list.len(), 3);
        assert_eq!(pages.bytes(), before - (FIRST_BLOCK + 200 * WORD));
        // a の添字に移った c も続けて返せる
        unsafe {
            pages.free(c);
            pages.free(b);
        }
        assert_eq!(pages.list.len(), 1);
        assert_eq!(blocks(&pages), 1);
    }

    #[test]
    fn the_prefix_shifts_the_header_and_the_class() {
        let mut pages = Pages::new(1);
        // 前置きを足して 17 語なので、20 語の級になる
        let a = pages.alloc(16);
        assert_eq!(unsafe { pages.block_words(a) }, 20);
        let offset = a.addr().get() - page_of(a).addr().get() - FIRST_BLOCK;
        assert_eq!(offset % (20 * WORD), WORD);
        assert!(unsafe { pages.fits(a, 16) });
        assert!(unsafe { pages.fits(a, 19) });
        assert!(!unsafe { pages.fits(a, 15) });
        assert!(!unsafe { pages.fits(a, 20) });
    }
}
