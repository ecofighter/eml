//! ページ単位の確保器 (docs/spec/object-model.md の「ページ」と「確保と解放」)。物体は
//! 64 KiB に揃えたページから切り出し、大きさはヘッダでなくページの頭が持つ。ヘッダの中身はここでは読まない。
//!
//! 権限の持ち方 (docs/spec/object-model.md の「`unsafe` の核」): ページの頭と切り出していない残りの権限は `Pages` が持つ。
//! ブロックの権限は、空きリストにある間は `Pages` が持ち、`alloc` で呼ぶ側に渡し、`free` で受け取る。切り出したブロックの
//! 集合とページごとの数は ghost の値で持ち、割り算を量化子の中に置かずに済ませる。

use std::alloc::{Layout, alloc, dealloc, handle_alloc_error};
use std::mem::ManuallyDrop;
use std::ptr;
use std::sync::Arc;

use vstd::prelude::*;
use vstd::raw_ptr::{Dealloc, PointsTo, PointsToRaw, ptr_mut_write, ptr_ref};

use crate::control::Control;
#[cfg(verus_only)]
use crate::frozen::RegionPerms;
use crate::heap::TOO_LARGE;
#[cfg(verus_only)]
use crate::meta::field_ptr;
use crate::meta::{BlockPerm, HEADER_WORDS, Header, meta_ptr, slot_ptr};
use crate::word::Word;

verus! {

pub(crate) const PAGE_BYTES: usize = 0x1_0000;

const WORD: usize = 8;

/// 小さい物体の級のブロックの語の数 (ヘッダと、検査モードでは前置きを含む)。2 語から 16 語は1語刻みにする。
pub(crate) const CLASS_WORDS: [usize; 27] = [
    2,
    3,
    4,
    5,
    6,
    7,
    8,
    9,
    10,
    11,
    12,
    13,
    14,
    15,
    16,
    20,
    24,
    28,
    32,
    40,
    48,
    56,
    64,
    80,
    96,
    112,
    128,
];

const CLASSES: usize = 27;

pub(crate) const MAX_SMALL_WORDS: usize = 128;

/// `c` は、`w` 語のブロックを入れられる一番小さい級である。
pub(crate) open spec fn rounds_up(w: int, c: int) -> bool {
    0 <= c < 27 && CLASS_WORDS[c] >= w && (c == 0 || CLASS_WORDS[c - 1] < w)
}

/// ブロックの語の数から級を引く表。確保の速い道で級を探して回らないためである。
exec const CLASS_OF: [u8; 129]
    ensures
        forall|w: int| #![trigger CLASS_OF[w]] 0 <= w <= 128 ==> rounds_up(w, CLASS_OF[w] as int),
{
    let mut table: [u8; 129] = [0;129];
    let mut words: usize = 0;
    let mut class: usize = 0;
    while words <= MAX_SMALL_WORDS
        invariant
            words <= 129,
            class < 27,
            class == 0 || CLASS_WORDS[class - 1] < words,
            forall|w: int| #![trigger table[w]] 0 <= w < words ==> rounds_up(w, table[w] as int),
        decreases 129 - words,
    {
        assert(CLASS_WORDS[26] == 128);
        while CLASS_WORDS[class] < words
            invariant
                class < 27,
                words <= 128,
                class == 0 || CLASS_WORDS[class - 1] < words,
            decreases 27 - class,
        {
            class += 1;
        }
        table[words] = class as u8;
        words += 1;
    }
    table
}

/// 大きい物体のページの級。
const LARGE: u32 = 0xFFFF_FFFF;

/// `Layout` が受け付けるページの大きさの上限 (`isize::MAX` を `PAGE_BYTES` に揃えて切り下げたもの)。
const MAX_PAGE_BYTES: usize = 0x7FFF_FFFF_FFFF_0000;

/// 空いたブロックの `meta` (種類 `Free` の印。docs/spec/object-model.md の「生存の列挙」)。
pub(crate) const FREE_META: u32 = 0x0F00_0000;

#[inline]
fn class_of(block_words: usize) -> (r: Option<usize>)
    ensures
        block_words <= MAX_SMALL_WORDS ==> r is Some && rounds_up(block_words as int, r->0 as int),
        block_words > MAX_SMALL_WORDS ==> r is None,
{
    if block_words <= MAX_SMALL_WORDS {
        Some(CLASS_OF[block_words] as usize)
    } else {
        None
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct PageHeader {
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

global layout PageHeader is size == 32, align == 8;

/// ページの頭の大きさ。
pub(crate) proof fn lemma_header_size()
    ensures
        vstd::layout::size_of::<PageHeader>() == 32,
{
}

/// 最初のブロックの位置。ページの頭の大きさを語に揃えたもの。
const FIRST_BLOCK: usize = 32;

#[derive(Clone, Copy)]
struct Class {
    /// 空いたブロックの LIFO の先頭。次のブロックは、空いたブロックのペイロードの先頭の語にある。
    free: *mut Header,
    page: *mut PageHeader,
    cursor: *mut u8,
    end: *mut u8,
}

/// ページの権限。頭、返すための `Dealloc`、まだ切り出していない残り。
// 仕様だけで使う型。Verus の外のビルドでも型は残るので、使わないことの警告を止める
#[cfg_attr(not(verus_keep_ghost), allow(dead_code))]
pub(crate) tracked struct PagePerm {
    tracked header: PointsTo<PageHeader>,
    tracked dealloc: Dealloc,
    tracked tail: PointsToRaw,
}

#[cfg_attr(not(verus_keep_ghost), allow(dead_code))]
pub(crate) struct Pages {
    prefix_words: usize,
    /// ページを取るときと返すときに、そのバイト数を数える先。
    // `Drop` がページを返した後で `release_control` に渡して手放す。欄の後始末として `Arc` を手放す処理が展開されると、
    // 計測のプログラムのコード生成単位の分け方が変わり、文字列の操作と `release` が呼ぶ側に展開されなくなったためである
    // (`Heap` の `region` の欄と同じ。docs/implementation/benchmarks.md の「`eml_rt` のインライン化」)
    control: ManuallyDrop<Arc<Control>>,
    list: Vec<*mut PageHeader>,
    classes: [Class; 27],
    bytes: usize,
    pages: Tracked<Map<usize, PagePerm>>,
    /// 空きリストにあるブロックの権限。キーはヘッダの番地である。
    pub(crate) free_perms: Tracked<Map<usize, BlockPerm>>,
    /// 級ごとの空きリスト。先頭から順に、ヘッダの番地を持つ。
    free: Ghost<Seq<Seq<usize>>>,
    /// 呼ぶ側に渡したブロック。
    in_use: Ghost<Set<usize>>,
    /// 切り出したブロック (渡したものと空きリストにあるもの)。
    carved: Ghost<Set<usize>>,
    /// ページごとの、切り出したブロックの数。
    count: Ghost<Map<usize, nat>>,
    /// ブロックごとの、ページの中の番号。
    index: Ghost<Map<usize, nat>>,
}

/// 下位 16 ビットを落とした番地。
pub(crate) open spec fn page_base(a: usize) -> usize {
    (a - a % PAGE_BYTES) as usize
}

/// ブロックはどれもページの先頭から 64 KiB の内にあるので、下位 16 ビットを落とせばページの頭になる。
proof fn lemma_mask(a: usize)
    ensures
        a & !0xFFFFusize == page_base(a),
        page_base(a) % 0x1_0000 == 0,
        page_base(a) <= a < page_base(a) + 0x1_0000,
{
    assert(a & !0xFFFFusize == a - a % 0x1_0000) by (bit_vector);
}

/// ページの頭 `a` から 64 KiB の内の番地は、`a` をページの頭にする。
proof fn lemma_page_base(a: usize, x: usize)
    requires
        a % 0x1_0000 == 0,
        a <= x < a + 0x1_0000,
    ensures
        page_base(x) == a,
{
    assert(x % 0x1_0000 == x - a) by (nonlinear_arith)
        requires
            a % 0x1_0000 == 0,
            a <= x < a + 0x1_0000,
    ;
}

/// 凍結した領域のページの頭の権限 `headers` から見た、ブロック `h` のヘッダを指すポインタ。provenance はページの確保の
/// ものである。
pub(crate) open spec fn region_ptr(
    headers: Map<usize, PointsTo<PageHeader>>,
    h: usize,
) -> *mut Header {
    vstd::raw_ptr::ptr_mut_from_data::<Header>(
        vstd::raw_ptr::PtrData {
            addr: h,
            provenance: headers[page_base(h)].ptr()@.provenance,
            metadata: (),
        },
    )
}

/// 凍結した領域のページの頭の権限 `headers` から見た、ブロック `h` のペイロードの語の数。`prefixed` なら前置きの1語を除く。
pub(crate) open spec fn region_cap(
    headers: Map<usize, PointsTo<PageHeader>>,
    h: usize,
    prefixed: bool,
) -> nat {
    header_cap(headers[page_base(h)].value(), prefixed)
}

/// ページの頭 `hdr` のページのブロックの、ペイロードの語の数。`prefixed` なら前置きの1語を除く。
pub(crate) closed spec fn header_cap(hdr: PageHeader, prefixed: bool) -> nat {
    (hdr.block_words - (if prefixed {
        1int
    } else {
        0int
    }) - HEADER_WORDS) as nat
}

/// ページの頭 `hdr` のページのブロックは、ヘッダと前置きと1語のペイロードが入る大きさである。
pub(crate) closed spec fn header_ok(hdr: PageHeader, prefixed: bool) -> bool {
    hdr.block_words >= (if prefixed {
        1int
    } else {
        0int
    }) + HEADER_WORDS + 1
}

/// ページの頭の権限を `pages` から `keys` の分だけ取り出す。頭のほかの権限は、ページを返す信頼した `Drop` が要らないので捨てる。
proof fn take_header_perms(
    tracked pages: &mut Map<usize, PagePerm>,
    keys: Set<usize>,
) -> (tracked r: Map<usize, PointsTo<PageHeader>>)
    requires
        keys.subset_of(old(pages).dom()),
    ensures
        r.dom() == keys,
        forall|a: usize| #[trigger] keys.contains(a) ==> r[a] == old(pages)[a].header,
    decreases keys.len(),
{
    if keys.len() == 0 {
        assert(keys =~= Set::<usize>::empty());
        Map::tracked_empty()
    } else {
        let a = keys.choose();
        let tracked page = pages.tracked_remove(a);
        let tracked mut r = take_header_perms(pages, keys.remove(a));
        let tracked PagePerm { header, dealloc, tail } = page;
        r.tracked_insert(a, header);
        assert(r.dom() =~= keys);
        r
    }
}

/// `keys` のページの頭は、どれも凍結した塊の鎖 `r` のページでない。
proof fn pages_apart(
    tracked pages: &mut Map<usize, PagePerm>,
    keys: Set<usize>,
    tracked r: &RegionPerms,
    prefixed: bool,
)
    requires
        keys.subset_of(old(pages).dom()),
        forall|a: usize| #[trigger] keys.contains(a) ==> old(pages)[a].header.ptr()@.addr == a,
        r.wf(prefixed),
    ensures
        *final(pages) == *old(pages),
        forall|a: usize| #[trigger] keys.contains(a) ==> !r.has_page(a),
    decreases keys.len(),
{
    if keys.len() > 0 {
        let a = keys.choose();
        let tracked mut page = pages.tracked_remove(a);
        r.lemma_apart(&mut page.header, prefixed);
        pages.tracked_insert(a, page);
        assert(*pages =~= *old(pages));
        pages_apart(pages, keys.remove(a), r, prefixed);
        assert forall|b: usize| #[trigger] keys.contains(b) implies !r.has_page(b) by {
            if b != a {
                assert(keys.remove(a).contains(b));
            }
        }
    } else {
        assert(keys =~= Set::<usize>::empty());
    }
}

#[inline]
fn page_of(h: *mut Header) -> (p: *mut PageHeader)
    ensures
        p@.addr == page_base(h@.addr),
        p@.provenance == h@.provenance,
{
    proof {
        lemma_mask(h@.addr);
    }
    (h as *mut PageHeader).with_addr(h.addr() & !(PAGE_BYTES - 1))
}

impl Pages {
    // 仕様
    pub(crate) closed spec fn prefixed(&self) -> bool {
        self.prefix_words == 1
    }

    pub(crate) closed spec fn in_use(&self) -> Set<usize> {
        self.in_use@
    }

    closed spec fn hdr(&self, a: usize) -> PageHeader {
        self.pages@[a].header.value()
    }

    closed spec fn page_provenance(&self, a: usize) -> vstd::raw_ptr::Provenance {
        self.pages@[a].dealloc.provenance()
    }

    closed spec fn block_bytes_of(&self, a: usize) -> int {
        self.hdr(a).block_words * 8
    }

    /// ページ `a` の切り出した終わり。今のページではヒープの欄から読む。
    closed spec fn limit_of(&self, a: usize) -> int {
        let c = self.hdr(a).class;
        if (c as int) < CLASSES && self.classes[c as int].page@.addr == a {
            self.classes[c as int].cursor@.addr - a
        } else {
            self.hdr(a).limit as int
        }
    }

    /// ページ `a` の i 番目のブロックのヘッダの番地。
    closed spec fn block_at(&self, a: usize, i: int) -> usize {
        (a + FIRST_BLOCK + i * self.block_bytes_of(a) + self.prefix_words * 8) as usize
    }

    /// `h` は、切り出したブロックのヘッダの番地である。
    pub(crate) closed spec fn carved_block(&self, h: usize) -> bool {
        self.carved@.contains(h)
    }

    /// ブロック `h` のヘッダを指すポインタ。provenance はページの確保のものである。
    pub(crate) closed spec fn block_ptr(&self, h: usize) -> *mut Header {
        vstd::raw_ptr::ptr_mut_from_data::<Header>(
            vstd::raw_ptr::PtrData {
                addr: h,
                provenance: self.page_provenance(page_base(h)),
                metadata: (),
            },
        )
    }

    /// ブロックの語の数 (前置きを含む)。
    pub(crate) closed spec fn block_words_spec(&self, h: usize) -> nat {
        self.hdr(page_base(h)).block_words as nat
    }

    /// ブロックのペイロードの語の数。
    pub(crate) closed spec fn cap(&self, h: usize) -> nat {
        (self.block_words_spec(h) - self.prefix_words - HEADER_WORDS) as nat
    }

    pub(crate) closed spec fn large(&self, h: usize) -> bool {
        self.hdr(page_base(h)).class == LARGE
    }

    /// ブロックが使うバイト数。大きい物体ではページ全体である。
    pub(crate) closed spec fn block_bytes_spec(&self, h: usize) -> int {
        if self.large(h) {
            self.hdr(page_base(h)).bytes as int
        } else {
            (self.block_words_spec(h) * 8) as int
        }
    }

    pub(crate) closed spec fn wf(&self) -> bool {
        &&& self.prefix_words <= 1
        &&& self.list_wf()
        &&& self.pages_wf()
        &&& self.classes_wf()
        &&& self.free_wf()
        &&& self.carved_wf()
        &&& self.bytes == self.sum_bytes(self.list@)
    }

    #[verifier::opaque]
    closed spec fn list_wf(&self) -> bool {
        &&& forall|i: int|
            0 <= i < self.list@.len() ==> self.pages@.contains_key(#[trigger] self.list@[i]@.addr)
        &&& forall|a: usize| #[trigger]
            self.pages@.contains_key(a) ==> 0 <= self.hdr(a).index < self.list@.len()
                && self.list@[self.hdr(a).index as int]@.addr == a
        &&& forall|i: int|
            0 <= i < self.list@.len() ==> self.hdr(#[trigger] self.list@[i]@.addr).index == i
        &&& forall|i: int|
            0 <= i < self.list@.len() ==> #[trigger] self.list@[i]
                == self.pages@[self.list@[i]@.addr].header.ptr()
    }

    #[verifier::opaque]
    closed spec fn pages_wf(&self) -> bool {
        &&& self.count@.dom() == self.pages@.dom()
        &&& forall|a: usize| #[trigger] self.pages@.contains_key(a) ==> self.page_wf(a)
    }

    closed spec fn page_wf(&self, a: usize) -> bool {
        let perm = self.pages@[a];
        let hdr = self.hdr(a);
        let lim = self.limit_of(a);
        &&& a % PAGE_BYTES == 0
        &&& a > 0
        &&& perm.header.ptr()@.addr == a
        &&& perm.header.ptr()@.provenance == self.page_provenance(a)
        &&& perm.header.is_init()
        &&& perm.dealloc.addr() == a
        &&& perm.dealloc.size() == hdr.bytes
        &&& perm.dealloc.align() == PAGE_BYTES
        &&& perm.tail.provenance() == self.page_provenance(a)
        &&& a + hdr.bytes <= usize::MAX
        &&& hdr.bytes <= MAX_PAGE_BYTES
        &&& hdr.block_words >= self.prefix_words + HEADER_WORDS + 1
        &&& lim == FIRST_BLOCK + self.count@[a] * self.block_bytes_of(a)
        &&& (hdr.class as int) < CLASSES ==> {
            &&& hdr.block_words == CLASS_WORDS[hdr.class as int]
            &&& hdr.bytes == PAGE_BYTES
            &&& lim <= PAGE_BYTES
            &&& perm.tail.is_range(a + lim, PAGE_BYTES - lim)
        }
        &&& hdr.class == LARGE ==> {
            &&& hdr.block_words > MAX_SMALL_WORDS
            &&& hdr.bytes == FIRST_BLOCK + hdr.block_words * 8
            &&& self.count@[a] == 1
            &&& perm.tail.is_range(a + lim, 0)
        }
        &&& (hdr.class as int) < CLASSES || hdr.class == LARGE
    }

    #[verifier::opaque]
    closed spec fn classes_wf(&self) -> bool {
        forall|c: int| 0 <= c < CLASSES ==> #[trigger] self.class_wf(c)
    }

    closed spec fn class_wf(&self, c: int) -> bool {
        let st = self.classes[c];
        let a = st.page@.addr;
        &&& (st.page@.addr == 0 <==> st.cursor@.addr == 0)
        &&& st.page@.addr != 0 ==> {
            &&& self.pages@.contains_key(a)
            &&& self.hdr(a).class == c
            &&& st.page@.provenance == self.page_provenance(a)
            &&& st.cursor@.provenance == self.page_provenance(a)
            &&& st.end@.addr == a + PAGE_BYTES
            &&& a + FIRST_BLOCK <= st.cursor@.addr <= a + PAGE_BYTES
        }
        &&& self.free@[c].len() == 0 ==> st.free@.addr == 0
        &&& self.free@[c].len() > 0 ==> st.free == self.block_ptr(self.free@[c][0])
    }

    #[verifier::opaque]
    closed spec fn free_wf(&self) -> bool {
        &&& self.free@.len() == CLASSES
        &&& forall|c: int, i: int|
            0 <= c < CLASSES && 0 <= i < self.free@[c].len() ==> #[trigger] self.free_entry(c, i)
        &&& forall|c: int| 0 <= c < CLASSES ==> (#[trigger] self.free@[c]).no_duplicates()
        &&& forall|h: usize| #[trigger] self.free_perms@.contains_key(h) ==> self.on_free_list(h)
    }

    /// 級 `c` の空きリストの i 番目のブロックは、その級の空いたブロックで、次のブロックを先頭の語に持つ。
    closed spec fn free_entry(&self, c: int, i: int) -> bool {
        let h = self.free@[c][i];
        let perm = self.free_perms@[h];
        &&& self.free_perms@.contains_key(h)
        &&& self.carved@.contains(h)
        &&& self.hdr(page_base(h)).class == c
        &&& perm.wf(self.block_ptr(h), self.cap(h), self.prefixed())
        &&& perm.meta.is_init() && perm.meta.value() == FREE_META
        &&& perm.slots[0].is_init()
        &&& i + 1 < self.free@[c].len() ==> perm.slots[0].value().ptr_spec()@.addr
            == self.free@[c][i + 1] && perm.slots[0].value().ptr_spec()@.provenance
            == self.page_provenance(page_base(self.free@[c][i + 1]))
        &&& i + 1 == self.free@[c].len() ==> perm.slots[0].value().ptr_spec()@.addr == 0
    }

    /// `h` はどれかの級の空きリストにある。級はページの頭が持つので、番号を1つに決められる。
    closed spec fn on_free_list(&self, h: usize) -> bool {
        let c = self.hdr(page_base(h)).class as int;
        0 <= c < CLASSES && self.free@[c].contains(h)
    }

    /// 切り出したブロックは、ページの中の番号で決まる位置にあり、渡したか空きリストにあるかのどちらか1つである。
    #[verifier::opaque]
    closed spec fn carved_wf(&self) -> bool {
        &&& forall|h: usize| #[trigger]
            self.carved@.contains(h) ==> {
                let a = page_base(h);
                &&& self.pages@.contains_key(a)
                &&& self.index@.contains_key(h)
                &&& self.index@[h] < self.count@[a]
                &&& h == self.block_at(a, self.index@[h] as int)
            }
        &&& forall|a: usize, i: int|
            self.pages@.contains_key(a) && 0 <= i < self.count@[a] ==> self.carved@.contains(
                #[trigger] self.block_at(a, i),
            )
        &&& forall|h: usize| #[trigger]
            self.carved@.contains(h) <==> (self.in_use@.contains(h)
                || self.free_perms@.contains_key(h))
        &&& forall|h: usize|
            self.in_use@.contains(h) ==> !(#[trigger] self.free_perms@.contains_key(h))
    }

    /// 一覧の j 番目のページは、頭の権限を持ち、不変条件を満たす。
    closed spec fn listed(&self, j: int) -> bool {
        let a = self.list@[j]@.addr;
        &&& self.pages@.contains_key(a)
        &&& self.page_wf(a)
        &&& self.list@[j] == self.pages@[a].header.ptr()
        &&& self.hdr(a).index == j
    }

    /// 切り出したブロック `h` は、一覧にあるページの、番号で決まる位置にある。
    closed spec fn located(&self, h: usize) -> bool {
        let a = page_base(h);
        &&& self.pages@.contains_key(a)
        &&& self.index@.contains_key(h)
        &&& self.index@[h] < self.count@[a]
        &&& h == self.block_at(a, self.index@[h] as int)
        &&& 0 <= self.hdr(a).index < self.list@.len()
        &&& self.list@[self.hdr(a).index as int]@.addr == a
    }

    /// ページのバイト数の和。`bytes` の欄はこの値に等しい。
    closed spec fn sum_bytes(&self, list: Seq<*mut PageHeader>) -> int
        decreases list.len(),
    {
        if list.len() == 0 {
            0
        } else {
            self.sum_bytes(list.drop_last()) + self.hdr(list.last()@.addr).bytes
        }
    }

    /// `alloc` の後の状態。`h` は新しく渡したブロックで、ほかの渡したブロックの形は変わらない。
    pub(crate) open spec fn alloc_post(
        &self,
        old: Pages,
        words: usize,
        h: *mut Header,
        perm: BlockPerm,
    ) -> bool {
        &&& self.alloc_shape(old, words, h)
        &&& perm.wf(h, self.cap(h@.addr), self.prefixed())
    }

    pub(crate) open spec fn alloc_shape(&self, old: Pages, words: usize, h: *mut Header) -> bool {
        &&& self.wf()
        &&& self.prefixed() == old.prefixed()
        &&& !old.in_use().contains(h@.addr)
        &&& self.in_use() == old.in_use().insert(h@.addr)
        &&& self.carved_block(h@.addr)
        &&& h == self.block_ptr(h@.addr)
        &&& self.cap(h@.addr) + HEADER_WORDS >= words
        &&& forall|b: usize| #[trigger] old.in_use().contains(b) ==> self.same_block(old, b)
    }

    /// ブロック `b` の形 (ポインタ、語の数、大きい物体か) が `old` と同じで、まだ切り出したブロックである。
    pub(crate) open spec fn same_block(&self, old: Pages, b: usize) -> bool {
        &&& self.block_ptr(b) == old.block_ptr(b)
        &&& self.block_words_spec(b) == old.block_words_spec(b)
        &&& self.cap(b) == old.cap(b)
        &&& self.large(b) == old.large(b)
        &&& self.block_bytes_spec(b) == old.block_bytes_spec(b)
        &&& self.carved_block(b)
    }

    /// ページ `a` の頭の値 `hdr` は、級、ブロックの語の数、バイト数、一覧の添字が今のページの頭と同じで、切り出した
    /// 終わりは今の位置である。凍結した領域のページの頭は、この形で自分のページを表す。
    pub(crate) closed spec fn describes(&self, a: usize, hdr: PageHeader) -> bool {
        &&& hdr.class == self.hdr(a).class
        &&& hdr.block_words == self.hdr(a).block_words
        &&& hdr.bytes == self.hdr(a).bytes
        &&& hdr.index == self.hdr(a).index
        &&& hdr.limit == self.limit_of(a)
    }

    // 補題
    /// 切り出したブロックは、語に揃い、前置きとペイロードの全体がアドレス空間に収まる。
    pub(crate) proof fn lemma_block_bounds(&self, h: usize)
        requires
            self.wf(),
            self.carved_block(h),
        ensures
            h % 8 == 0,
            h >= 8,
            h + 8 * (HEADER_WORDS + self.cap(h)) <= usize::MAX,
            self.cap(h) >= 1,
            self.block_ptr(h)@.addr == h,
    {
        self.lemma_block(h);
        vstd::raw_ptr::axiom_ptr_mut_from_data(
            vstd::raw_ptr::PtrData::<Header> {
                addr: h,
                provenance: self.page_provenance(page_base(h)),
                metadata: (),
            },
        );
    }

    /// ブロック `h` はページの中にある。
    proof fn lemma_block(&self, h: usize)
        requires
            self.wf(),
            self.carved_block(h),
        ensures
            self.pages@.contains_key(page_base(h)),
            self.page_wf(page_base(h)),
            page_base(h) + FIRST_BLOCK + self.prefix_words * 8 <= h,
            h - self.prefix_words * 8 + self.block_bytes_of(page_base(h)) <= page_base(h)
                + self.limit_of(page_base(h)),
            page_base(h) + self.limit_of(page_base(h)) <= page_base(h) + self.hdr(
                page_base(h),
            ).bytes,
            page_base(h) + self.hdr(page_base(h)).bytes <= usize::MAX,
            h % 8 == 0,
            self.cap(h) >= 1,
            self.cap(h) == self.hdr(page_base(h)).block_words - self.prefix_words - HEADER_WORDS,
            (self.hdr(page_base(h)).class as int) < CLASSES ==> self.hdr(page_base(h)).block_words
                <= MAX_SMALL_WORDS,
            self.hdr(page_base(h)).class == LARGE ==> self.index@[h] == 0,
    {
        reveal(Pages::carved_wf);
        reveal(Pages::pages_wf);
        let a = page_base(h);
        assert(self.page_wf(a));
        let bb = self.block_bytes_of(a);
        let k = self.index@[h] as int;
        let n = self.count@[a] as int;
        let bw = self.hdr(a).block_words as int;
        assert(k * bb + bb <= n * bb) by (nonlinear_arith)
            requires
                k + 1 <= n,
                bb > 0,
        ;
        assert(a % 8 == 0) by (nonlinear_arith)
            requires
                a % 0x1_0000 == 0,
        ;
        assert(k * bb == (k * bw) * 8) by (nonlinear_arith)
            requires
                bb == bw * 8,
        ;
        vstd::arithmetic::div_mod::lemma_mod_multiples_basic(k * bw, 8);
        assert(k * bb >= 0) by (nonlinear_arith)
            requires
                k >= 0,
                bb > 0,
        ;
        if (self.hdr(a).class as int) < CLASSES {
            assert(CLASS_WORDS[self.hdr(a).class as int] <= 128);
        }
    }

    /// ページ `a` の i 番目のブロックは、そのページを頭にする。
    proof fn lemma_block_at(&self, a: usize, i: int)
        requires
            self.prefix_words <= 1,
            self.page_wf(a),
            0 <= i < self.count@[a],
        ensures
            page_base(self.block_at(a, i)) == a,
            self.block_at(a, i) == a + FIRST_BLOCK + i * self.block_bytes_of(a) + self.prefix_words
                * 8,
    {
        let bb = self.block_bytes_of(a);
        let n = self.count@[a] as int;
        assert(i * bb + bb <= n * bb) by (nonlinear_arith)
            requires
                i + 1 <= n,
                bb > 0,
        ;
        assert(i * bb >= 0) by (nonlinear_arith)
            requires
                i >= 0,
                bb > 0,
        ;
        lemma_page_base(a, (a + FIRST_BLOCK + i * bb + self.prefix_words * 8) as usize);
    }

    /// ページの頭、級の今のページと切り出しの位置、前置きが同じなら、ページの不変条件と切り出したブロックの形は変わらない。
    proof fn lemma_layout_frame(&self, s0: Pages)
        requires
            self.pages == s0.pages,
            self.count == s0.count,
            self.prefix_words == s0.prefix_words,
            forall|c: int|
                0 <= c < CLASSES ==> #[trigger] self.classes[c].page == s0.classes[c].page
                    && self.classes[c].cursor == s0.classes[c].cursor,
        ensures
            self.pages_wf() == s0.pages_wf(),
            forall|a: usize| #[trigger] self.limit_of(a) == s0.limit_of(a),
            forall|a: usize, i: int| #[trigger] self.block_at(a, i) == s0.block_at(a, i),
            forall|h: usize| #[trigger] self.block_ptr(h) == s0.block_ptr(h),
            forall|h: usize| #[trigger] self.block_words_spec(h) == s0.block_words_spec(h),
            forall|h: usize| #[trigger] self.cap(h) == s0.cap(h),
            forall|h: usize| #[trigger] self.large(h) == s0.large(h),
            forall|h: usize| #[trigger] self.block_bytes_spec(h) == s0.block_bytes_spec(h),
    {
        reveal(Pages::pages_wf);
        assert forall|a: usize| #[trigger] self.limit_of(a) == s0.limit_of(a) by {
            let c = self.hdr(a).class;
            if (c as int) < CLASSES {
                assert(self.classes[c as int].page == s0.classes[c as int].page);
            }
        }
        assert forall|a: usize| self.pages@.contains_key(a) implies #[trigger] self.page_wf(a)
            == s0.page_wf(a) by {
            assert(self.limit_of(a) == s0.limit_of(a));
        }
    }

    /// 和は、列のページの頭のバイト数が同じなら変わらない。
    proof fn lemma_sum_frame(&self, other: &Pages, list: Seq<*mut PageHeader>)
        requires
            forall|i: int|
                0 <= i < list.len() ==> self.hdr(#[trigger] list[i]@.addr).bytes == other.hdr(
                    list[i]@.addr,
                ).bytes,
        ensures
            self.sum_bytes(list) == other.sum_bytes(list),
        decreases list.len(),
    {
        if list.len() > 0 {
            self.lemma_sum_frame(other, list.drop_last());
        }
    }

    /// 列の i 番目を x に替えると、和はその差だけ変わる。
    proof fn lemma_sum_update(&self, list: Seq<*mut PageHeader>, i: int, x: *mut PageHeader)
        requires
            0 <= i < list.len(),
        ensures
            self.sum_bytes(list.update(i, x)) == self.sum_bytes(list) - self.hdr(
                list[i]@.addr,
            ).bytes + self.hdr(x@.addr).bytes,
        decreases list.len(),
    {
        let n = list.len();
        if i == n - 1 {
            assert(list.update(i, x).drop_last() =~= list.drop_last());
        } else {
            assert(list.update(i, x).drop_last() =~= list.drop_last().update(i, x));
            self.lemma_sum_update(list.drop_last(), i, x);
        }
    }

    /// 列の i 番目を最後の項目で置き換えて最後を外すと (`swap_remove`)、和は i 番目のページのバイト数だけ減る。
    proof fn lemma_sum_swap_remove(&self, list: Seq<*mut PageHeader>, i: int)
        requires
            0 <= i < list.len(),
        ensures
            self.sum_bytes(list.update(i, list.last()).drop_last()) == self.sum_bytes(list)
                - self.hdr(list[i]@.addr).bytes,
    {
        let n = list.len();
        if i == n - 1 {
            assert(list.update(i, list.last()).drop_last() =~= list.drop_last());
        } else {
            assert(list.update(i, list.last()).drop_last() =~= list.drop_last().update(
                i,
                list.last(),
            ));
            self.lemma_sum_update(list.drop_last(), i, list.last());
        }
    }

    // 実行
    pub(crate) fn new(prefix_words: usize, control: Arc<Control>) -> (r: Pages)
        requires
            prefix_words <= 1,
        ensures
            r.wf(),
            r.prefixed() == (prefix_words == 1),
            r.in_use() == Set::<usize>::empty(),
    {
        let empty = Class {
            free: ptr::null_mut(),
            page: ptr::null_mut(),
            cursor: ptr::null_mut(),
            end: ptr::null_mut(),
        };
        let r = Pages {
            prefix_words,
            control: ManuallyDrop::new(control),
            list: Vec::new(),
            classes: [empty;27],
            bytes: 0,
            pages: Tracked(Map::tracked_empty()),
            free_perms: Tracked(Map::tracked_empty()),
            free: Ghost(Seq::new(27, |c: int| Seq::<usize>::empty())),
            in_use: Ghost(Set::empty()),
            carved: Ghost(Set::empty()),
            count: Ghost(Map::empty()),
            index: Ghost(Map::empty()),
        };
        proof {
            reveal(Pages::list_wf);
            reveal(Pages::pages_wf);
            reveal(Pages::classes_wf);
            reveal(Pages::free_wf);
            reveal(Pages::carved_wf);
            assert(r.count@.dom() =~= r.pages@.dom());
        }
        r
    }

    /// 各級の今のページの切り出しの位置をページの頭に書き戻してから、ページの頭の権限を取り出す。凍結した領域はブロックの
    /// 位置と大きさをページの頭から読むためである。この後の `Pages` は、ページを返す `Drop` にだけ使う。
    pub(crate) fn take_headers(&mut self) -> (r: Tracked<Map<usize, PointsTo<PageHeader>>>)
        requires
            old(self).wf(),
        ensures
            forall|h: usize| #[trigger]
                old(self).carved_block(h) ==> {
                    &&& r@.contains_key(page_base(h))
                    &&& region_ptr(r@, h) == old(self).block_ptr(h)
                    &&& region_cap(r@, h, old(self).prefixed()) == old(self).cap(h)
                },
            forall|a: usize| #[trigger]
                r@.contains_key(a) ==> {
                    &&& r@[a].ptr()@.addr == a
                    &&& r@[a].is_init()
                    &&& old(self).describes(a, r@[a].value())
                    &&& header_ok(r@[a].value(), old(self).prefixed())
                },
    {
        let ghost s0 = *self;
        proof {
            reveal(Pages::pages_wf);
            reveal(Pages::classes_wf);
        }
        let mut c: usize = 0;
        while c < CLASSES
            invariant
                s0.wf(),
                c <= CLASSES,
                self.classes == s0.classes,
                self.pages@.dom() == s0.pages@.dom(),
                forall|a: usize| #[trigger]
                    self.pages@.contains_key(a) ==> {
                        let hdr = self.pages@[a].header.value();
                        &&& self.pages@[a].header.ptr() == s0.pages@[a].header.ptr()
                        &&& self.pages@[a].header.is_init()
                        &&& hdr.class == s0.hdr(a).class
                        &&& hdr.block_words == s0.hdr(a).block_words
                        &&& hdr.bytes == s0.hdr(a).bytes
                        &&& hdr.index == s0.hdr(a).index
                        &&& hdr.limit == if (s0.hdr(a).class as int) < c && s0.classes[s0.hdr(
                            a,
                        ).class as int].page@.addr == a {
                            s0.limit_of(a)
                        } else {
                            s0.hdr(a).limit as int
                        }
                    },
            decreases CLASSES - c,
        {
            let st = self.classes[c];
            proof {
                reveal(Pages::pages_wf);
                reveal(Pages::classes_wf);
                assert(s0.class_wf(c as int));
            }
            if st.page.addr() != 0 {
                let ghost a = st.page@.addr;
                proof {
                    assert(s0.page_wf(a));
                }
                let mut hdr = self.header_of(st.page);
                hdr.limit = st.cursor.addr() - st.page.addr();
                let tracked mut perm = self.pages.borrow_mut().tracked_remove(a);
                ptr_mut_write(st.page, Tracked(&mut perm.header), hdr);
                proof {
                    self.pages.borrow_mut().tracked_insert(a, perm);
                    assert(self.pages@.dom() =~= s0.pages@.dom());
                }
            }
            c += 1;
        }
        proof {
            assert forall|h: usize| #[trigger] s0.carved_block(h) implies s0.pages@.contains_key(
                page_base(h),
            ) && s0.page_wf(page_base(h)) by {
                s0.lemma_block(h);
            }
            assert forall|a: usize| #[trigger]
                s0.pages@.contains_key(a) implies self.pages@[a].header.value().limit
                == s0.limit_of(a) by {
                assert(s0.page_wf(a));
            }
        }
        let ghost keys = self.pages@.dom();
        let ghost s1 = *self;
        let tracked headers = take_header_perms(self.pages.borrow_mut(), keys);
        proof {
            assert forall|a: usize| #[trigger] headers.contains_key(a) implies header_ok(
                headers[a].value(),
                s0.prefixed(),
            ) by {
                assert(s0.page_wf(a));
                assert(headers[a] == s1.pages@[a].header);
            }
        }
        Tracked(headers)
    }

    /// OS から取ったページのバイト数。
    pub(crate) fn bytes(&self) -> usize {
        self.bytes
    }

    pub(crate) fn control(&self) -> &Arc<Control> {
        &self.control
    }

    #[inline]
    fn header_of(&self, page: *mut PageHeader) -> (r: PageHeader)
        requires
            self.pages@.contains_key(page@.addr),
            self.pages@[page@.addr].header.ptr() == page,
            self.pages@[page@.addr].header.is_init(),
        ensures
            r == self.hdr(page@.addr),
    {
        let tracked perm = self.pages.borrow().tracked_borrow(page@.addr);
        *ptr_ref(page, Tracked(&perm.header))
    }

    /// ブロックの頭を読む。
    #[inline]
    fn block_header(&self, h: *mut Header) -> (r: PageHeader)
        requires
            self.wf(),
            self.carved_block(h@.addr),
            h == self.block_ptr(h@.addr),
        ensures
            r == self.hdr(page_base(h@.addr)),
    {
        let page = page_of(h);
        proof {
            self.lemma_block(h@.addr);
        }
        self.header_of(page)
    }

    /// ブロックの語の数 (前置きを含む)。
    #[inline]
    pub(crate) fn block_words(&self, h: *mut Header) -> (r: usize)
        requires
            self.wf(),
            self.carved_block(h@.addr),
            h == self.block_ptr(h@.addr),
        ensures
            r == self.block_words_spec(h@.addr),
    {
        self.block_header(h).block_words as usize
    }

    /// ブロックのペイロードの語の数。
    #[inline]
    pub(crate) fn cap_of(&self, h: *mut Header) -> (r: usize)
        requires
            self.wf(),
            self.carved_block(h@.addr),
            h == self.block_ptr(h@.addr),
        ensures
            r == self.cap(h@.addr),
    {
        let tracked hdr = self.tracked_block_header(h@.addr);
        self.cap_in(h, Tracked(hdr))
    }

    /// 切り出したブロック `h` のページの頭の権限を借りる。
    pub(crate) proof fn tracked_block_header(tracked &self, h: usize) -> (tracked r: &PointsTo<
        PageHeader,
    >)
        requires
            self.wf(),
            self.carved_block(h),
        ensures
            r.ptr()@.addr == page_base(h),
            r.ptr()@.provenance == self.block_ptr(h)@.provenance,
            r.is_init(),
            r.value() == self.page_header(page_base(h)),
            header_ok(r.value(), self.prefixed()),
            header_cap(r.value(), self.prefixed()) == self.cap(h),
    {
        self.lemma_block(h);
        &self.pages.borrow().tracked_borrow(page_base(h)).header
    }

    /// ページの頭 `hdr` のページ `page_base(h)` は、この確保器のページである。頭の `index` は持ち主の一覧の添字なので、
    /// 一覧のその項がこのページならこの確保器のページである。凍結した塊のページの `index` は、塊を作ったヒープの一覧の
    /// 添字なので、この一覧の項は別のページになる。ブロックを並べずに、ヒープのブロックと塊のブロックを見分けるのに使う。
    #[inline]
    pub(crate) fn owns(&self, h: *mut Header, Tracked(hdr): Tracked<&PointsTo<PageHeader>>) -> (r:
        bool)
        requires
            self.wf(),
            hdr.ptr()@.addr == page_base(h@.addr),
            hdr.ptr()@.provenance == h@.provenance,
            hdr.is_init(),
        ensures
            r ==> self.has_page(page_base(h@.addr)),
            self.has_page(page_base(h@.addr)) && hdr.value() == self.page_header(page_base(h@.addr))
                ==> r,
    {
        proof {
            reveal(Pages::list_wf);
        }
        let page = page_of(h);
        let i = ptr_ref(page, Tracked(hdr)).index;
        let r = i < self.list.len() && self.list[i].addr() == page.addr();
        proof {
            let a = page_base(h@.addr);
            if r {
                assert(self.pages@.contains_key(self.list@[i as int]@.addr));
            }
            if self.has_page(a) && hdr.value() == self.page_header(a) {
                assert(self.pages@.contains_key(a));
                assert(self.list@[self.hdr(a).index as int]@.addr == a);
            }
        }
        r
    }

    /// ページの頭の権限 `hdr` で、ブロック `h` のペイロードの語の数を読む。`hdr` はこの確保器のページの頭か、凍結した
    /// 塊のページの頭である。どちらでも同じ読み方をする。
    #[inline]
    pub(crate) fn cap_in(&self, h: *mut Header, Tracked(hdr): Tracked<&PointsTo<PageHeader>>) -> (r:
        usize)
        requires
            self.wf(),
            hdr.ptr()@.addr == page_base(h@.addr),
            hdr.ptr()@.provenance == h@.provenance,
            hdr.is_init(),
            header_ok(hdr.value(), self.prefixed()),
        ensures
            r == header_cap(hdr.value(), self.prefixed()),
    {
        let page = page_of(h);
        let hd = *ptr_ref(page, Tracked(hdr));
        hd.block_words as usize - self.prefix_words - HEADER_WORDS
    }

    /// ブロックが使うバイト数。大きい物体ではページ全体である。検査モードの隔離の数え方に使う。
    #[inline]
    pub(crate) fn block_bytes(&self, h: *mut Header) -> (r: usize)
        requires
            self.wf(),
            self.carved_block(h@.addr),
            h == self.block_ptr(h@.addr),
        ensures
            r == self.block_bytes_spec(h@.addr),
    {
        let hdr = self.block_header(h);
        proof {
            self.lemma_block(h@.addr);
        }
        if hdr.class == LARGE {
            hdr.bytes
        } else {
            hdr.block_words as usize * WORD
        }
    }

    /// 箱が `words` 語の物体の級か (token の検査)。
    #[inline]
    pub(crate) fn fits(&self, h: *mut Header, words: usize) -> (r: bool)
        requires
            self.wf(),
            self.carved_block(h@.addr),
            h == self.block_ptr(h@.addr),
        ensures
            r ==> self.cap(h@.addr) + HEADER_WORDS >= words,
    {
        let block_words = match words.checked_add(self.prefix_words) {
            Some(n) => n,
            None => too_large(),
        };
        let want = match class_of(block_words) {
            Some(class) => CLASS_WORDS[class],
            None => block_words,
        };
        proof {
            self.lemma_block(h@.addr);
        }
        self.block_words(h) == want
    }

    /// `words` 語 (ヘッダを含み、前置きを含まない) の物体のブロックを取り、ヘッダの位置と権限を返す。ヘッダは書かない。
    #[inline]
    pub(crate) fn alloc(&mut self, words: usize) -> (r: (*mut Header, Tracked<BlockPerm>))
        requires
            old(self).wf(),
            words >= HEADER_WORDS + 1,
        ensures
            final(self).alloc_post(*old(self), words, r.0, r.1@),
    {
        let block_words = match words.checked_add(self.prefix_words) {
            Some(n) => n,
            None => too_large(),
        };
        match class_of(block_words) {
            Some(class) => {
                if self.classes[class].free.addr() != 0 {
                    self.pop_free(class, Ghost(words))
                } else {
                    self.carve(class, Ghost(words))
                }
            },
            None => self.alloc_large(block_words, Ghost(words)),
        }
    }

    /// 空きリストの先頭を取る。
    #[inline]
    fn pop_free(&mut self, class: usize, Ghost(words): Ghost<usize>) -> (r: (
        *mut Header,
        Tracked<BlockPerm>,
    ))
        requires
            old(self).wf(),
            class < CLASSES,
            CLASS_WORDS[class as int] >= words + old(self).prefix_words,
            words >= HEADER_WORDS + 1,
            old(self).classes[class as int].free@.addr != 0,
        ensures
            final(self).alloc_post(*old(self), words, r.0, r.1@),
    {
        let ghost s0 = *self;
        let st = self.classes[class];
        let h = st.free;
        proof {
            reveal(Pages::classes_wf);
            reveal(Pages::free_wf);
            assert(self.class_wf(class as int));
            assert(self.free@[class as int].len() > 0);
            assert(self.free_entry(class as int, 0));
            self.lemma_block(h@.addr);
        }
        let tracked perm = self.free_perms.borrow_mut().tracked_remove(h@.addr);
        let next = *ptr_ref(slot_ptr(h, 0), Tracked(perm.slots.tracked_borrow(0)));
        let mut st2 = st;
        st2.free = next.as_ptr();
        self.classes[class] = st2;
        proof {
            let c = class as int;
            let n = self.free@[c].len();
            self.free = Ghost(self.free@.update(c, self.free@[c].subrange(1, n as int)));
            self.in_use = Ghost(self.in_use@.insert(h@.addr));
            self.lemma_pop(s0, c, h, words);
        }
        (h, Tracked(perm))
    }

    proof fn lemma_pop(&self, s0: Pages, c: int, h: *mut Header, words: usize)
        requires
            s0.wf(),
            0 <= c < CLASSES,
            CLASS_WORDS[c] >= words + s0.prefix_words,
            s0.free@[c].len() > 0,
            h@.addr == s0.free@[c][0],
            h == s0.classes[c].free,
            s0.free_entry(c, 0),
            self.pages == s0.pages,
            self.list == s0.list,
            self.bytes == s0.bytes,
            self.prefix_words == s0.prefix_words,
            self.carved == s0.carved,
            self.count == s0.count,
            self.index == s0.index,
            self.classes[c].page == s0.classes[c].page,
            self.classes[c].cursor == s0.classes[c].cursor,
            self.classes[c].end == s0.classes[c].end,
            self.classes[c].free@.addr == s0.free_perms@[h@.addr].slots[0].value().ptr_spec()@.addr,
            self.classes[c].free@.provenance
                == s0.free_perms@[h@.addr].slots[0].value().ptr_spec()@.provenance,
            forall|c2: int|
                0 <= c2 < CLASSES && c2 != c ==> #[trigger] self.classes[c2] == s0.classes[c2],
            self.free@ == s0.free@.update(c, s0.free@[c].subrange(1, s0.free@[c].len() as int)),
            self.in_use@ == s0.in_use@.insert(h@.addr),
            self.free_perms@ == s0.free_perms@.remove(h@.addr),
        ensures
            self.alloc_post(s0, words, h, s0.free_perms@[h@.addr]),
    {
        let hs = h@.addr;
        let rest = s0.free@[c].subrange(1, s0.free@[c].len() as int);
        assert forall|c2: int| 0 <= c2 < CLASSES implies #[trigger] self.classes[c2].page
            == s0.classes[c2].page && self.classes[c2].cursor == s0.classes[c2].cursor by {
            if c2 != c {
                assert(self.classes[c2] == s0.classes[c2]);
            }
        }
        self.lemma_layout_frame(s0);
        self.lemma_sum_frame(&s0, self.list@);
        reveal(Pages::list_wf);
        reveal(Pages::classes_wf);
        reveal(Pages::free_wf);
        reveal(Pages::carved_wf);
        assert(s0.free@[c].no_duplicates());
        assert forall|c2: int| 0 <= c2 < CLASSES implies #[trigger] self.class_wf(c2) by {
            assert(s0.class_wf(c2));
            if c2 == c && rest.len() > 0 {
                assert(s0.free_entry(c, 0));
                assert(self.classes[c].free@ == s0.block_ptr(rest[0])@);
            }
        }
        assert forall|c2: int, i: int|
            0 <= c2 < CLASSES && 0 <= i < self.free@[c2].len() implies #[trigger] self.free_entry(
            c2,
            i,
        ) by {
            if c2 == c {
                assert(s0.free_entry(c, i + 1));
                assert(self.free@[c][i] == s0.free@[c][i + 1]);
                assert(s0.free@[c][i + 1] != hs);
            } else {
                assert(s0.free_entry(c2, i));
                let h2 = self.free@[c2][i];
                if h2 == hs {
                    assert(s0.on_free_list(hs));
                }
            }
        }
        assert forall|c2: int| 0 <= c2 < CLASSES implies (
        #[trigger] self.free@[c2]).no_duplicates() by {
            assert(s0.free@[c2].no_duplicates());
        }
        assert forall|h2: usize| #[trigger]
            self.free_perms@.contains_key(h2) implies self.on_free_list(h2) by {
            assert(s0.on_free_list(h2));
            let c2 = s0.hdr(page_base(h2)).class as int;
            if c2 == c {
                let k = choose|k: int| 0 <= k < s0.free@[c].len() && s0.free@[c][k] == h2;
                assert(k != 0);
                assert(rest[k - 1] == h2);
            }
        }
        assert(self.free_wf());
        assert(self.carved_wf());
        assert(self.classes_wf());
        assert(self.list_wf());
        s0.lemma_block(hs);
        assert(s0.free_entry(c, 0));
        assert(self.pages_wf());
        assert(self.wf());
        assert(!s0.in_use().contains(hs));
        assert(self.in_use() =~= s0.in_use().insert(hs));
        assert(self.carved_block(hs));
        assert(s0.class_wf(c));
        assert(h == self.block_ptr(hs));
        assert(s0.free_perms@[hs].wf(h, self.cap(hs), self.prefixed()));
        assert(s0.hdr(page_base(hs)).class == c);
        reveal(Pages::pages_wf);
        assert(s0.page_wf(page_base(hs)));
        assert(self.cap(hs) + HEADER_WORDS >= words);
        assert forall|b: usize| #[trigger] s0.in_use().contains(b) implies self.same_block(
            s0,
            b,
        ) by {
            assert(s0.carved@.contains(b));
        }
    }

    /// 級の今のページから新しいブロックを切り出す。ページに余りがなければ、ページを替える。
    #[inline]
    #[verifier::spinoff_prover]
    fn carve(&mut self, class: usize, Ghost(words): Ghost<usize>) -> (r: (
        *mut Header,
        Tracked<BlockPerm>,
    ))
        requires
            old(self).wf(),
            class < CLASSES,
            CLASS_WORDS[class as int] >= words + old(self).prefix_words,
            words >= HEADER_WORDS + 1,
        ensures
            final(self).alloc_post(*old(self), words, r.0, r.1@),
    {
        let block_bytes = CLASS_WORDS[class] * WORD;
        let st = self.classes[class];
        proof {
            reveal(Pages::classes_wf);
            assert(self.class_wf(class as int));
        }
        if st.cursor.addr() == 0 || st.end.addr() - st.cursor.addr() < block_bytes {
            self.next_page(class);
        }
        let ghost s1 = *self;
        proof {
            assert forall|b: usize| #[trigger] old(self).carved_block(b) implies s1.same_block(
                *old(self),
                b,
            ) by {}
        }
        let st = self.classes[class];
        let ghost a = st.page@.addr;
        proof {
            reveal(Pages::classes_wf);
            reveal(Pages::pages_wf);
            assert(self.class_wf(class as int));
            assert(self.page_wf(a));
            self.lemma_cursor(class as int);
        }
        let block = st.cursor;
        let h = (block as *mut Header).with_addr(block.addr() + self.prefix_words * WORD);
        let tracked pp = self.pages.borrow_mut().tracked_remove(a);
        let tracked PagePerm { header, dealloc, tail } = pp;
        let ghost lo = block@.addr as int;
        let tracked (raw, rest) = tail.split(Set::range(lo, lo + block_bytes));
        proof {
            assert(rest.dom() =~= Set::range(lo + block_bytes, a + PAGE_BYTES));
        }
        let ghost cap = (CLASS_WORDS[class as int] - self.prefix_words - HEADER_WORDS) as nat;
        let tracked perm = BlockPerm::from_raw(raw, h, cap, self.prefix_words == 1);
        proof {
            self.pages.borrow_mut().tracked_insert(a, PagePerm { header, dealloc, tail: rest });
        }
        let mut st2 = st;
        st2.cursor = block.with_addr(block.addr() + block_bytes);
        self.classes[class] = st2;
        proof {
            let n = self.count@[a];
            self.in_use = Ghost(self.in_use@.insert(h@.addr));
            self.carved = Ghost(self.carved@.insert(h@.addr));
            self.count = Ghost(self.count@.insert(a, n + 1));
            self.index = Ghost(self.index@.insert(h@.addr, n));
            self.lemma_carve(s1, *old(self), class as int, h, words);
        }
        (h, Tracked(perm))
    }

    /// 級の今のページの切り出しの位置は、語に揃い、次のブロックの番号の位置にある。
    proof fn lemma_cursor(&self, c: int)
        requires
            self.prefix_words <= 1,
            0 <= c < CLASSES,
            self.class_wf(c),
            self.classes[c].page@.addr != 0,
            self.page_wf(self.classes[c].page@.addr),
        ensures
            self.classes[c].cursor@.addr % 8 == 0,
            self.classes[c].cursor@.addr == self.classes[c].page@.addr + FIRST_BLOCK
                + self.count@[self.classes[c].page@.addr] * self.block_bytes_of(
                self.classes[c].page@.addr,
            ),
            self.limit_of(self.classes[c].page@.addr) == self.classes[c].cursor@.addr
                - self.classes[c].page@.addr,
    {
        let a = self.classes[c].page@.addr;
        let bw = self.hdr(a).block_words as int;
        let n = self.count@[a] as int;
        assert(a % 8 == 0) by (nonlinear_arith)
            requires
                a % 0x1_0000 == 0,
        ;
        assert(n * (bw * 8) == (n * bw) * 8) by (nonlinear_arith);
        vstd::arithmetic::div_mod::lemma_mod_multiples_basic(n * bw, 8);
    }

    proof fn lemma_carve(&self, s1: Pages, s0: Pages, c: int, h: *mut Header, words: usize)
        requires
            s0.wf(),
            s1.wf(),
            0 <= c < CLASSES,
            CLASS_WORDS[c] >= words + s0.prefix_words,
            words >= HEADER_WORDS + 1,
            s1.prefix_words == s0.prefix_words,
            s1.in_use == s0.in_use,
            forall|b: usize| #[trigger] s0.carved_block(b) ==> s1.same_block(s0, b),
            s1.carved == s0.carved,
            s1.classes[c].page@.addr != 0,
            s1.classes[c].end@.addr - s1.classes[c].cursor@.addr >= CLASS_WORDS[c] * 8,
            ({
                let a = s1.classes[c].page@.addr;
                let lo = s1.classes[c].cursor@.addr;
                &&& h@.addr == lo + s1.prefix_words * 8
                &&& h@.provenance == s1.classes[c].cursor@.provenance
                &&& self.pages@.dom() == s1.pages@.dom()
                &&& self.pages@[a].header == s1.pages@[a].header
                &&& self.pages@[a].dealloc == s1.pages@[a].dealloc
                &&& self.pages@[a].tail.is_range(
                    lo + CLASS_WORDS[c] * 8,
                    a + PAGE_BYTES - (lo + CLASS_WORDS[c] * 8),
                )
                &&& self.pages@[a].tail.provenance() == s1.pages@[a].tail.provenance()
                &&& forall|x: usize| x != a ==> #[trigger] self.pages@[x] == s1.pages@[x]
                &&& self.classes[c].cursor@.addr == lo + CLASS_WORDS[c] * 8
                &&& self.classes[c].cursor@.provenance == s1.classes[c].cursor@.provenance
                &&& self.count@ == s1.count@.insert(a, s1.count@[a] + 1)
                &&& self.index@ == s1.index@.insert(h@.addr, s1.count@[a])
            }),
            self.classes[c].page == s1.classes[c].page,
            self.classes[c].end == s1.classes[c].end,
            self.classes[c].free == s1.classes[c].free,
            forall|c2: int|
                0 <= c2 < CLASSES && c2 != c ==> #[trigger] self.classes[c2] == s1.classes[c2],
            self.list == s1.list,
            self.bytes == s1.bytes,
            self.prefix_words == s1.prefix_words,
            self.free == s1.free,
            self.free_perms == s1.free_perms,
            self.in_use@ == s1.in_use@.insert(h@.addr),
            self.carved@ == s1.carved@.insert(h@.addr),
        ensures
            self.alloc_shape(s0, words, h),
            self.cap(h@.addr) == CLASS_WORDS[c] - s0.prefix_words - HEADER_WORDS,
    {
        let a = s1.classes[c].page@.addr;
        let lo = s1.classes[c].cursor@.addr as int;
        let n = s1.count@[a] as int;
        let bb = CLASS_WORDS[c] * 8;
        let hs = h@.addr;
        reveal(Pages::list_wf);
        reveal(Pages::pages_wf);
        reveal(Pages::classes_wf);
        reveal(Pages::free_wf);
        reveal(Pages::carved_wf);
        assert(s1.class_wf(c));
        assert(s1.page_wf(a));
        s1.lemma_cursor(c);
        assert(s1.block_bytes_of(a) == bb);
        // 新しいブロックは、ページの n 番目のブロックである
        assert(lo + bb <= a + PAGE_BYTES);
        lemma_page_base(a, hs);
        assert(hs == s1.block_at(a, n));
        assert(n * bb + bb == (n + 1) * bb) by (nonlinear_arith);
        // 前には切り出していない
        assert(!s1.carved@.contains(hs)) by {
            if s1.carved@.contains(hs) {
                let k = s1.index@[hs] as int;
                assert(page_base(hs) == a);
                assert(k < n);
                assert(hs == s1.block_at(a, k));
                assert(0 <= k * bb <= n * bb) by (nonlinear_arith)
                    requires
                        0 <= k < n,
                        bb > 0,
                ;
                assert(k * bb == n * bb);
                assert(k == n) by (nonlinear_arith)
                    requires
                        k * bb == n * bb,
                        bb > 0,
                ;
            }
        }
        // ページの頭は変わらない
        assert forall|x: usize| #[trigger]
            self.hdr(x) == s1.hdr(x) && self.page_provenance(x) == s1.page_provenance(x) by {
            if x != a {
                assert(self.pages@[x] == s1.pages@[x]);
            }
        }
        assert forall|x: usize| x != a implies #[trigger] self.limit_of(x) == s1.limit_of(x) by {
            let cx = self.hdr(x).class;
            if (cx as int) < CLASSES && cx as int != c {
                assert(self.classes[cx as int] == s1.classes[cx as int]);
            }
        }
        assert(self.limit_of(a) == FIRST_BLOCK + (n + 1) * bb);
        assert forall|x: usize| #[trigger] self.pages@.contains_key(x) implies self.page_wf(x) by {
            if x != a {
                assert(s1.page_wf(x));
            }
        }
        assert(self.count@.dom() =~= self.pages@.dom());
        assert forall|c2: int| 0 <= c2 < CLASSES implies #[trigger] self.class_wf(c2) by {
            assert(s1.class_wf(c2));
            if c2 != c {
                assert(self.classes[c2] == s1.classes[c2]);
            }
        }
        assert forall|c2: int, i: int|
            0 <= c2 < CLASSES && 0 <= i < self.free@[c2].len() implies #[trigger] self.free_entry(
            c2,
            i,
        ) by {
            assert(s1.free_entry(c2, i));
        }
        assert forall|h2: usize| #[trigger]
            self.free_perms@.contains_key(h2) implies self.on_free_list(h2) by {
            assert(s1.on_free_list(h2));
        }
        assert forall|x: usize, i: int|
            self.pages@.contains_key(x) && 0 <= i < self.count@[x] implies self.carved@.contains(
            #[trigger] self.block_at(x, i),
        ) by {
            assert(self.block_at(x, i) == s1.block_at(x, i));
            if x == a && i == n {
                assert(self.block_at(x, i) == hs);
            }
        }
        assert forall|h2: usize| #[trigger] self.carved@.contains(h2) implies ({
            let x = page_base(h2);
            &&& self.pages@.contains_key(x)
            &&& self.index@.contains_key(h2)
            &&& self.index@[h2] < self.count@[x]
            &&& h2 == self.block_at(x, self.index@[h2] as int)
        }) by {
            if h2 != hs {
                assert(s1.carved@.contains(h2));
                assert(self.block_at(page_base(h2), s1.index@[h2] as int) == s1.block_at(
                    page_base(h2),
                    s1.index@[h2] as int,
                ));
            }
        }
        assert forall|h2: usize| #[trigger]
            self.carved@.contains(h2) <==> (self.in_use@.contains(h2)
                || self.free_perms@.contains_key(h2)) by {
            assert(s1.carved@.contains(h2) <==> (s1.in_use@.contains(h2)
                || s1.free_perms@.contains_key(h2)));
        }
        assert(self.carved_wf());
        assert(self.free_wf());
        assert(self.classes_wf());
        assert(self.pages_wf());
        assert(self.list_wf());
        self.lemma_sum_frame(&s1, self.list@);
        assert(self.wf());
        assert(self.in_use() =~= s0.in_use().insert(hs));
        assert(h == self.block_ptr(hs));
        assert forall|b: usize| #[trigger] s0.in_use().contains(b) implies self.same_block(
            s0,
            b,
        ) by {
            assert(s0.carved_block(b));
            assert(s1.same_block(s0, b));
        }
    }

    /// 級 `class` の今のページを替える。前のページの切り出した終わりを、ページの頭に書き戻す。
    #[cold]
    fn next_page(&mut self, class: usize)
        requires
            old(self).wf(),
            class < CLASSES,
            CLASS_WORDS[class as int] >= old(self).prefix_words + HEADER_WORDS + 1,
        ensures
            final(self).wf(),
            final(self).prefix_words == old(self).prefix_words,
            final(self).in_use == old(self).in_use,
            final(self).carved == old(self).carved,
            final(self).index == old(self).index,
            final(self).free_perms == old(self).free_perms,
            final(self).free == old(self).free,
            final(self).classes[class as int].page@.addr != 0,
            final(self).classes[class as int].end@.addr
                - final(self).classes[class as int].cursor@.addr >= PAGE_BYTES - FIRST_BLOCK,
            forall|b: usize| #[trigger]
                old(self).carved_block(b) ==> final(self).same_block(*old(self), b),
    {
        let ghost s0 = *self;
        let st = self.classes[class];
        proof {
            reveal(Pages::classes_wf);
            reveal(Pages::pages_wf);
            assert(self.class_wf(class as int));
        }
        if st.page.addr() != 0 {
            proof {
                assert(self.page_wf(st.page@.addr));
            }
            let mut hdr = self.header_of(st.page);
            hdr.limit = st.cursor.addr() - st.page.addr();
            let tracked mut perm = self.pages.borrow_mut().tracked_remove(st.page@.addr);
            ptr_mut_write(st.page, Tracked(&mut perm.header), hdr);
            proof {
                self.pages.borrow_mut().tracked_insert(st.page@.addr, perm);
            }
        }
        let ghost s1 = *self;
        proof {
            assert forall|a: usize| #[trigger]
                s1.pages@.contains_key(a) implies s1.pages@[a].header.ptr()@.addr == a by {
                assert(s0.page_wf(a));
            }
        }
        let (page, Tracked(perm)) = self.fresh_page(PAGE_BYTES, class as u32, CLASS_WORDS[class]);
        proof {
            self.pages.borrow_mut().tracked_insert(page@.addr, perm);
            self.count = Ghost(self.count@.insert(page@.addr, 0));
        }
        let base = page as *mut u8;
        self.classes[class] = Class {
            free: st.free,
            page,
            cursor: base.with_addr(base.addr() + FIRST_BLOCK),
            end: base.with_addr(base.addr() + PAGE_BYTES),
        };
        proof {
            self.lemma_next_page(s0, s1, class as int, page);
        }
    }

    proof fn lemma_next_page(&self, s0: Pages, s1: Pages, c: int, p: *mut PageHeader)
        requires
            s0.wf(),
            0 <= c < CLASSES,
            CLASS_WORDS[c] >= s0.prefix_words + HEADER_WORDS + 1,
            ({
                let a0 = s0.classes[c].page@.addr;
                &&& a0 != 0 ==> s1.pages@.dom() == s0.pages@.dom() && s1.hdr(a0) == (PageHeader {
                    limit: (s0.classes[c].cursor@.addr - a0) as usize,
                    ..s0.hdr(a0)
                }) && s1.pages@[a0].dealloc == s0.pages@[a0].dealloc && s1.pages@[a0].tail
                    == s0.pages@[a0].tail && s1.pages@[a0].header.ptr()
                    == s0.pages@[a0].header.ptr() && s1.pages@[a0].header.is_init() && forall|
                    x: usize,
                |
                    x != a0 ==> #[trigger] s1.pages@[x] == s0.pages@[x]
                &&& a0 == 0 ==> s1.pages == s0.pages
            }),
            !s0.pages@.contains_key(p@.addr),
            p@.addr % PAGE_BYTES == 0,
            p@.addr > 0,
            p@.addr + PAGE_BYTES <= usize::MAX,
            self.pages@ == s1.pages@.insert(p@.addr, self.pages@[p@.addr]),
            self.pages@[p@.addr].header.ptr() == p,
            self.pages@[p@.addr].header.is_init(),
            self.hdr(p@.addr) == (PageHeader {
                class: c as u32,
                block_words: CLASS_WORDS[c] as u32,
                index: s0.list@.len() as usize,
                limit: FIRST_BLOCK,
                bytes: PAGE_BYTES,
            }),
            self.pages@[p@.addr].dealloc.addr() == p@.addr,
            self.pages@[p@.addr].dealloc.size() == PAGE_BYTES,
            self.pages@[p@.addr].dealloc.align() == PAGE_BYTES,
            self.pages@[p@.addr].dealloc.provenance() == p@.provenance,
            self.pages@[p@.addr].tail.is_range(p@.addr + FIRST_BLOCK, PAGE_BYTES - FIRST_BLOCK),
            self.pages@[p@.addr].tail.provenance() == p@.provenance,
            self.list@ == s0.list@.push(p),
            s0.list@.len() <= usize::MAX,
            self.bytes == s0.bytes + PAGE_BYTES,
            self.count@ == s0.count@.insert(p@.addr, 0),
            self.carved == s0.carved,
            self.index == s0.index,
            self.free_perms == s0.free_perms,
            self.free == s0.free,
            self.in_use == s0.in_use,
            self.prefix_words == s0.prefix_words,
            self.classes[c].free == s0.classes[c].free,
            self.classes[c].page == p,
            self.classes[c].cursor@.addr == p@.addr + FIRST_BLOCK,
            self.classes[c].cursor@.provenance == p@.provenance,
            self.classes[c].end@.addr == p@.addr + PAGE_BYTES,
            forall|c2: int|
                0 <= c2 < CLASSES && c2 != c ==> #[trigger] self.classes[c2] == s0.classes[c2],
        ensures
            self.wf(),
            forall|b: usize| #[trigger] s0.carved_block(b) ==> self.same_block(s0, b),
    {
        let a0 = s0.classes[c].page@.addr;
        let pa = p@.addr;
        reveal(Pages::list_wf);
        reveal(Pages::pages_wf);
        reveal(Pages::classes_wf);
        reveal(Pages::free_wf);
        reveal(Pages::carved_wf);
        // 古いページの頭は、今のページの切り出しの位置を書き戻した欄のほかは変わらない
        assert forall|x: usize| s0.pages@.contains_key(x) implies #[trigger] self.hdr(x).class
            == s0.hdr(x).class && self.hdr(x).block_words == s0.hdr(x).block_words && self.hdr(
            x,
        ).bytes == s0.hdr(x).bytes && self.hdr(x).index == s0.hdr(x).index && self.page_provenance(
            x,
        ) == s0.page_provenance(x) && self.pages@[x].dealloc == s0.pages@[x].dealloc
            && self.pages@[x].tail == s0.pages@[x].tail && self.pages@[x].header.ptr()
            == s0.pages@[x].header.ptr() && self.limit_of(x) == s0.limit_of(x) by {
            assert(x != pa);
            assert(s0.page_wf(x));
            assert(self.pages@[x] == s1.pages@[x]);
            let cx = s0.hdr(x).class;
            if a0 != 0 && x == a0 {
                assert(s0.class_wf(c));
                assert(s0.hdr(x).class == c);
                assert(s0.limit_of(x) == s0.classes[c].cursor@.addr - a0);
            } else {
                assert(s1.pages@[x] == s0.pages@[x]);
                if (cx as int) < CLASSES {
                    assert(s0.class_wf(cx as int));
                    if cx as int == c {
                        assert(s0.classes[c].page@.addr != x);
                    } else {
                        assert(self.classes[cx as int] == s0.classes[cx as int]);
                    }
                }
            }
        }
        assert forall|x: usize| #[trigger] self.pages@.contains_key(x) implies self.page_wf(x) by {
            if x != pa {
                assert(s0.page_wf(x));
            }
        }
        assert(self.count@.dom() =~= self.pages@.dom());
        assert forall|c2: int| 0 <= c2 < CLASSES implies #[trigger] self.class_wf(c2) by {
            assert(s0.class_wf(c2));
            if s0.free@[c2].len() > 0 {
                let f = s0.free@[c2][0];
                assert(s0.free_entry(c2, 0));
                assert(s0.carved@.contains(f));
            }
        }
        assert forall|h: usize| #[trigger] s0.carved@.contains(h) implies page_base(h) != pa
            && self.block_ptr(h) == s0.block_ptr(h) && self.cap(h) == s0.cap(h)
            && self.block_words_spec(h) == s0.block_words_spec(h) && self.large(h) == s0.large(h)
            && self.block_bytes_spec(h) == s0.block_bytes_spec(h) by {
            assert(s0.pages@.contains_key(page_base(h)));
        }
        assert forall|c2: int, i: int|
            0 <= c2 < CLASSES && 0 <= i < self.free@[c2].len() implies #[trigger] self.free_entry(
            c2,
            i,
        ) by {
            assert(s0.free_entry(c2, i));
            if i + 1 < self.free@[c2].len() {
                assert(s0.free_entry(c2, i + 1));
            }
        }
        assert forall|h2: usize| #[trigger]
            self.free_perms@.contains_key(h2) implies self.on_free_list(h2) by {
            assert(s0.on_free_list(h2));
            assert(s0.carved@.contains(h2));
        }
        assert forall|x: usize, i: int|
            self.pages@.contains_key(x) && 0 <= i < self.count@[x] implies self.carved@.contains(
            #[trigger] self.block_at(x, i),
        ) by {
            assert(x != pa);
            assert(s0.block_at(x, i) == self.block_at(x, i));
        }
        assert forall|h: usize| #[trigger] self.carved@.contains(h) implies ({
            let a = page_base(h);
            &&& self.pages@.contains_key(a)
            &&& self.index@.contains_key(h)
            &&& self.index@[h] < self.count@[a]
            &&& h == self.block_at(a, self.index@[h] as int)
        }) by {
            assert(s0.pages@.contains_key(page_base(h)));
        }
        assert(self.carved_wf());
        assert(self.free_wf());
        assert forall|i: int| 0 <= i < s0.list@.len() implies self.hdr(
            #[trigger] s0.list@[i]@.addr,
        ).bytes == s0.hdr(s0.list@[i]@.addr).bytes by {
            assert(s0.pages@.contains_key(s0.list@[i]@.addr));
        }
        self.lemma_sum_frame(&s0, s0.list@);
        assert(self.list@.drop_last() =~= s0.list@);
        let n = s0.list@.len();
        assert forall|i: int| 0 <= i < self.list@.len() implies self.pages@.contains_key(
            #[trigger] self.list@[i]@.addr,
        ) by {
            if i < n {
                assert(self.list@[i] == s0.list@[i]);
                assert(s0.pages@.contains_key(s0.list@[i]@.addr));
            }
        }
        assert forall|a: usize| #[trigger] self.pages@.contains_key(a) implies 0 <= self.hdr(
            a,
        ).index < self.list@.len() && self.list@[self.hdr(a).index as int]@.addr == a by {
            if a != pa {
                assert(s0.pages@.contains_key(a));
                assert(self.hdr(a).class == s0.hdr(a).class);
                assert(self.hdr(a).index == s0.hdr(a).index);
                assert(self.list@[s0.hdr(a).index as int] == s0.list@[s0.hdr(a).index as int]);
            }
        }
        assert forall|i: int| 0 <= i < self.list@.len() implies self.hdr(
            #[trigger] self.list@[i]@.addr,
        ).index == i by {
            if i < n {
                assert(self.list@[i] == s0.list@[i]);
                assert(s0.pages@.contains_key(s0.list@[i]@.addr));
                assert(self.hdr(s0.list@[i]@.addr).class == s0.hdr(s0.list@[i]@.addr).class);
            }
        }
        assert forall|i: int| 0 <= i < self.list@.len() implies #[trigger] self.list@[i]
            == self.pages@[self.list@[i]@.addr].header.ptr() by {
            if i < n {
                assert(self.list@[i] == s0.list@[i]);
                assert(s0.pages@.contains_key(s0.list@[i]@.addr));
                assert(self.hdr(s0.list@[i]@.addr).class == s0.hdr(s0.list@[i]@.addr).class);
            }
        }
        assert(self.list_wf());
    }

    /// 新しいページを OS から取り、頭を書いて一覧に足す。権限はまだ `pages` に入れずに返す。
    #[cold]
    fn fresh_page(&mut self, bytes: usize, class: u32, block_words: usize) -> (r: (
        *mut PageHeader,
        Tracked<PagePerm>,
    ))
        requires
            forall|a: usize| #[trigger]
                old(self).pages@.contains_key(a) ==> old(self).pages@[a].header.ptr()@.addr == a,
            FIRST_BLOCK < bytes,
        ensures
            r.0@.addr % PAGE_BYTES == 0,
            r.0@.addr > 0,
            r.0@.addr + bytes <= usize::MAX,
            bytes <= MAX_PAGE_BYTES,
            block_words <= 0xFFFF_FFFF,
            !old(self).pages@.contains_key(r.0@.addr),
            r.1@.header.ptr() == r.0,
            r.1@.header.is_init(),
            r.1@.header.value() == (PageHeader {
                class,
                block_words: block_words as u32,
                index: old(self).list@.len() as usize,
                limit: FIRST_BLOCK,
                bytes,
            }),
            r.1@.dealloc.addr() == r.0@.addr,
            r.1@.dealloc.size() == bytes,
            r.1@.dealloc.align() == PAGE_BYTES,
            r.1@.dealloc.provenance() == r.0@.provenance,
            r.1@.tail.is_range(r.0@.addr + FIRST_BLOCK, bytes - FIRST_BLOCK),
            r.1@.tail.provenance() == r.0@.provenance,
            final(self).list@ == old(self).list@.push(r.0),
            old(self).list@.len() <= usize::MAX,
            final(self).bytes == old(self).bytes + bytes,
            final(self).pages == old(self).pages,
            final(self).classes == old(self).classes,
            final(self).free_perms == old(self).free_perms,
            final(self).free == old(self).free,
            final(self).in_use == old(self).in_use,
            final(self).carved == old(self).carved,
            final(self).count == old(self).count,
            final(self).index == old(self).index,
            final(self).prefix_words == old(self).prefix_words,
    {
        if block_words > 0xFFFF_FFFF {
            too_many_words();
        }
        if bytes > MAX_PAGE_BYTES {
            bad_page_layout();
        }
        let total = match self.bytes.checked_add(bytes) {
            Some(n) => n,
            None => too_large(),
        };
        let (base, Tracked(raw), Tracked(dealloc)) = alloc_page(bytes);
        charge_pages(&self.control, bytes);
        let page = base as *mut PageHeader;
        let ghost a = base@.addr as int;
        let tracked (head, tail) = raw.split(Set::range(a, a + FIRST_BLOCK));
        proof {
            assert(tail.dom() =~= Set::range(a + FIRST_BLOCK, a + bytes));
            assert(a % 8 == 0) by (nonlinear_arith)
                requires
                    a % 0x1_0000 == 0,
            ;
        }
        let tracked mut header = head.into_typed::<PageHeader>(base@.addr);
        let hdr = PageHeader {
            class,
            block_words: block_words as u32,
            index: self.list.len(),
            limit: FIRST_BLOCK,
            bytes,
        };
        ptr_mut_write(page, Tracked(&mut header), hdr);
        proof {
            // ほかのページの頭の権限と同じ番地にはならない
            if self.pages@.contains_key(page@.addr) {
                let tracked other = self.pages.borrow().tracked_borrow(page@.addr);
                header.is_disjoint(&other.header);
            }
        }
        self.list.push(page);
        self.bytes = total;
        (page, Tracked(PagePerm { header, dealloc, tail }))
    }

    /// 大きい物体のブロックを、専用のページで取る。
    #[cold]
    #[verifier::spinoff_prover]
    fn alloc_large(&mut self, block_words: usize, Ghost(words): Ghost<usize>) -> (r: (
        *mut Header,
        Tracked<BlockPerm>,
    ))
        requires
            old(self).wf(),
            block_words == words + old(self).prefix_words,
            block_words > MAX_SMALL_WORDS,
        ensures
            final(self).alloc_post(*old(self), words, r.0, r.1@),
    {
        if block_words > (usize::MAX - FIRST_BLOCK) / WORD {
            too_large();
        }
        let bytes = block_words * WORD + FIRST_BLOCK;
        let ghost s0 = *self;
        proof {
            reveal(Pages::pages_wf);
            assert forall|a: usize| #[trigger]
                self.pages@.contains_key(a) implies self.pages@[a].header.ptr()@.addr == a by {
                assert(self.page_wf(a));
            }
        }
        let (page, Tracked(perm)) = self.fresh_page(bytes, LARGE, block_words);
        let tracked PagePerm { header: mut header, dealloc, tail } = perm;
        let mut hdr = *ptr_ref(page, Tracked(&header));
        hdr.limit = bytes;
        ptr_mut_write(page, Tracked(&mut header), hdr);
        let h = (page as *mut Header).with_addr(
            page.addr() + FIRST_BLOCK + self.prefix_words * WORD,
        );
        let ghost a = page@.addr as int;
        let tracked (raw, rest) = tail.split(Set::range(a + FIRST_BLOCK, a + bytes));
        proof {
            assert(rest.dom() =~= Set::range(a + bytes, a + bytes));
            assert(a % 8 == 0) by (nonlinear_arith)
                requires
                    a % 0x1_0000 == 0,
            ;
        }
        let ghost cap = (block_words - self.prefix_words - HEADER_WORDS) as nat;
        let tracked bp = BlockPerm::from_raw(raw, h, cap, self.prefix_words == 1);
        proof {
            self.pages.borrow_mut().tracked_insert(
                page@.addr,
                PagePerm { header, dealloc, tail: rest },
            );
            self.count = Ghost(self.count@.insert(page@.addr, 1));
            self.carved = Ghost(self.carved@.insert(h@.addr));
            self.index = Ghost(self.index@.insert(h@.addr, 0));
            self.in_use = Ghost(self.in_use@.insert(h@.addr));
            self.lemma_large(s0, page, h, block_words, words);
        }
        (h, Tracked(bp))
    }

    proof fn lemma_large(
        &self,
        s0: Pages,
        p: *mut PageHeader,
        h: *mut Header,
        block_words: usize,
        words: usize,
    )
        requires
            s0.wf(),
            block_words == words + s0.prefix_words,
            block_words > MAX_SMALL_WORDS,
            block_words <= 0xFFFF_FFFF,
            !s0.pages@.contains_key(p@.addr),
            p@.addr % PAGE_BYTES == 0,
            p@.addr > 0,
            p@.addr + FIRST_BLOCK + block_words * 8 <= usize::MAX,
            self.pages@ == s0.pages@.insert(p@.addr, self.pages@[p@.addr]),
            self.pages@[p@.addr].header.ptr() == p,
            self.pages@[p@.addr].header.is_init(),
            self.hdr(p@.addr) == (PageHeader {
                class: LARGE,
                block_words: block_words as u32,
                index: s0.list@.len() as usize,
                limit: (FIRST_BLOCK + block_words * 8) as usize,
                bytes: (FIRST_BLOCK + block_words * 8) as usize,
            }),
            self.pages@[p@.addr].dealloc.addr() == p@.addr,
            self.pages@[p@.addr].dealloc.size() == FIRST_BLOCK + block_words * 8,
            self.pages@[p@.addr].dealloc.align() == PAGE_BYTES,
            self.pages@[p@.addr].dealloc.provenance() == p@.provenance,
            self.pages@[p@.addr].tail.is_range(p@.addr + FIRST_BLOCK + block_words * 8, 0),
            self.pages@[p@.addr].tail.provenance() == p@.provenance,
            h@.addr == p@.addr + FIRST_BLOCK + s0.prefix_words * 8,
            h@.provenance == p@.provenance,
            self.list@ == s0.list@.push(p),
            s0.list@.len() <= usize::MAX,
            self.bytes == s0.bytes + FIRST_BLOCK + block_words * 8,
            self.count@ == s0.count@.insert(p@.addr, 1),
            self.carved@ == s0.carved@.insert(h@.addr),
            self.index@ == s0.index@.insert(h@.addr, 0),
            self.in_use@ == s0.in_use@.insert(h@.addr),
            self.free_perms == s0.free_perms,
            self.free == s0.free,
            self.prefix_words == s0.prefix_words,
            self.classes == s0.classes,
        ensures
            self.alloc_shape(s0, words, h),
            self.cap(h@.addr) == block_words - s0.prefix_words - HEADER_WORDS,
    {
        let pa = p@.addr;
        let hs = h@.addr;
        reveal(Pages::list_wf);
        reveal(Pages::pages_wf);
        reveal(Pages::classes_wf);
        reveal(Pages::free_wf);
        reveal(Pages::carved_wf);
        lemma_page_base(pa, hs);
        assert forall|x: usize| s0.pages@.contains_key(x) implies #[trigger] self.pages@[x]
            == s0.pages@[x] && x != pa by {}
        assert forall|x: usize| s0.pages@.contains_key(x) implies #[trigger] self.limit_of(x)
            == s0.limit_of(x) by {
            assert(self.pages@[x] == s0.pages@[x]);
        }
        assert(self.limit_of(pa) == FIRST_BLOCK + 1 * self.block_bytes_of(pa));
        assert forall|x: usize| #[trigger] self.pages@.contains_key(x) implies self.page_wf(x) by {
            if x != pa {
                assert(s0.page_wf(x));
                assert(self.pages@[x] == s0.pages@[x]);
            }
        }
        assert(self.count@.dom() =~= self.pages@.dom());
        assert forall|h2: usize| #[trigger] s0.carved@.contains(h2) implies page_base(h2) != pa
            && self.block_ptr(h2) == s0.block_ptr(h2) && self.cap(h2) == s0.cap(h2)
            && self.block_words_spec(h2) == s0.block_words_spec(h2) && self.large(h2) == s0.large(
            h2,
        ) && self.block_bytes_spec(h2) == s0.block_bytes_spec(h2) by {
            assert(s0.pages@.contains_key(page_base(h2)));
            assert(self.pages@[page_base(h2)] == s0.pages@[page_base(h2)]);
        }
        assert(!s0.carved@.contains(hs));
        assert forall|c2: int| 0 <= c2 < CLASSES implies #[trigger] self.class_wf(c2) by {
            assert(s0.class_wf(c2));
            if s0.classes[c2].page@.addr != 0 {
                assert(self.pages@[s0.classes[c2].page@.addr]
                    == s0.pages@[s0.classes[c2].page@.addr]);
            }
            if s0.free@[c2].len() > 0 {
                assert(s0.free_entry(c2, 0));
            }
        }
        assert forall|c2: int, i: int|
            0 <= c2 < CLASSES && 0 <= i < self.free@[c2].len() implies #[trigger] self.free_entry(
            c2,
            i,
        ) by {
            assert(s0.free_entry(c2, i));
            if i + 1 < self.free@[c2].len() {
                assert(s0.free_entry(c2, i + 1));
            }
        }
        assert forall|h2: usize| #[trigger]
            self.free_perms@.contains_key(h2) implies self.on_free_list(h2) by {
            assert(s0.on_free_list(h2));
            assert(s0.carved@.contains(h2));
        }
        assert forall|x: usize, i: int|
            self.pages@.contains_key(x) && 0 <= i < self.count@[x] implies self.carved@.contains(
            #[trigger] self.block_at(x, i),
        ) by {
            if x == pa {
                assert(i == 0);
                assert(0 * self.block_bytes_of(pa) == 0);
                assert(self.block_at(x, i) == hs);
            } else {
                assert(self.pages@[x] == s0.pages@[x]);
                assert(self.block_at(x, i) == s0.block_at(x, i));
            }
        }
        assert forall|h2: usize| #[trigger] self.carved@.contains(h2) implies ({
            let x = page_base(h2);
            &&& self.pages@.contains_key(x)
            &&& self.index@.contains_key(h2)
            &&& self.index@[h2] < self.count@[x]
            &&& h2 == self.block_at(x, self.index@[h2] as int)
        }) by {
            if h2 != hs {
                assert(s0.carved@.contains(h2));
                assert(self.pages@[page_base(h2)] == s0.pages@[page_base(h2)]);
                assert(self.block_at(page_base(h2), s0.index@[h2] as int) == s0.block_at(
                    page_base(h2),
                    s0.index@[h2] as int,
                ));
            }
        }
        assert(self.carved_wf());
        assert(self.free_wf());
        assert(self.classes_wf());
        assert(self.pages_wf());
        let n = s0.list@.len();
        assert forall|i: int| 0 <= i < self.list@.len() implies self.pages@.contains_key(
            #[trigger] self.list@[i]@.addr,
        ) by {
            if i < n {
                assert(self.list@[i] == s0.list@[i]);
            }
        }
        assert forall|x: usize| #[trigger] self.pages@.contains_key(x) implies 0 <= self.hdr(
            x,
        ).index < self.list@.len() && self.list@[self.hdr(x).index as int]@.addr == x by {
            if x != pa {
                assert(self.pages@[x] == s0.pages@[x]);
                assert(self.list@[s0.hdr(x).index as int] == s0.list@[s0.hdr(x).index as int]);
            }
        }
        assert forall|i: int| 0 <= i < self.list@.len() implies self.hdr(
            #[trigger] self.list@[i]@.addr,
        ).index == i by {
            if i < n {
                assert(self.list@[i] == s0.list@[i]);
                assert(self.pages@[s0.list@[i]@.addr] == s0.pages@[s0.list@[i]@.addr]);
            }
        }
        assert forall|i: int| 0 <= i < self.list@.len() implies #[trigger] self.list@[i]
            == self.pages@[self.list@[i]@.addr].header.ptr() by {
            if i < n {
                assert(self.list@[i] == s0.list@[i]);
                assert(self.pages@[s0.list@[i]@.addr] == s0.pages@[s0.list@[i]@.addr]);
            }
        }
        assert(self.list_wf());
        assert forall|i: int| 0 <= i < s0.list@.len() implies self.hdr(
            #[trigger] s0.list@[i]@.addr,
        ).bytes == s0.hdr(s0.list@[i]@.addr).bytes by {
            assert(self.pages@[s0.list@[i]@.addr] == s0.pages@[s0.list@[i]@.addr]);
        }
        self.lemma_sum_frame(&s0, s0.list@);
        assert(self.list@.drop_last() =~= s0.list@);
        assert(self.wf());
        assert(self.in_use() =~= s0.in_use().insert(hs));
        assert(h == self.block_ptr(hs));
        assert forall|b: usize| #[trigger] s0.in_use().contains(b) implies self.same_block(
            s0,
            b,
        ) by {
            assert(s0.carved@.contains(b));
        }
    }

    /// 速いモードで物体の箱を空ける。小さい物体のブロックは種類 `Free` の印と次のブロックを書いて空きリストに積む。
    /// 印を書くのは、速いモードでもページを歩いて生きている物体を見分けるためである (docs/spec/object-model.md の「生存の列挙」)。
    /// 大きい物体のページはすぐ返す。
    #[inline]
    pub(crate) fn free(&mut self, h: *mut Header, Tracked(perm): Tracked<BlockPerm>)
        requires
            old(self).wf(),
            old(self).in_use().contains(h@.addr),
            h == old(self).block_ptr(h@.addr),
            perm.wf(h, old(self).cap(h@.addr), old(self).prefixed()),
        ensures
            final(self).wf(),
            final(self).prefixed() == old(self).prefixed(),
            final(self).in_use() == old(self).in_use().remove(h@.addr),
            forall|b: usize| #[trigger]
                final(self).in_use().contains(b) ==> final(self).same_block(*old(self), b),
    {
        proof {
            reveal(Pages::carved_wf);
            self.lemma_block(h@.addr);
        }
        let hdr = self.block_header(h);
        if hdr.class == LARGE {
            self.release_large(h, Tracked(perm));
            return;
        }
        let ghost s0 = *self;
        let class = hdr.class as usize;
        proof {
            reveal(Pages::pages_wf);
            assert(s0.page_wf(page_base(h@.addr)));
        }
        let tracked mut perm = perm;
        ptr_mut_write(meta_ptr(h), Tracked(&mut perm.meta), FREE_META);
        let st = self.classes[class];
        let tracked mut slot = perm.slots.tracked_remove(0);
        ptr_mut_write(slot_ptr(h, 0), Tracked(&mut slot), Word::from_ptr(st.free));
        proof {
            perm.slots.tracked_insert(0, slot);
            assert(perm.wf(h, s0.cap(h@.addr), s0.prefixed()));
        }
        let mut st2 = st;
        st2.free = h;
        self.classes[class] = st2;
        proof {
            let c = class as int;
            self.free_perms.borrow_mut().tracked_insert(h@.addr, perm);
            self.free = Ghost(self.free@.update(c, seq![h@.addr] + self.free@[c]));
            self.in_use = Ghost(self.in_use@.remove(h@.addr));
            self.lemma_free(s0, c, h);
        }
    }

    proof fn lemma_free(&self, s0: Pages, c: int, h: *mut Header)
        requires
            s0.wf(),
            0 <= c < CLASSES,
            s0.in_use().contains(h@.addr),
            h == s0.block_ptr(h@.addr),
            s0.carved_block(h@.addr),
            s0.hdr(page_base(h@.addr)).class == c,
            self.pages == s0.pages,
            self.list == s0.list,
            self.bytes == s0.bytes,
            self.prefix_words == s0.prefix_words,
            self.carved == s0.carved,
            self.count == s0.count,
            self.index == s0.index,
            self.classes[c].page == s0.classes[c].page,
            self.classes[c].cursor == s0.classes[c].cursor,
            self.classes[c].end == s0.classes[c].end,
            self.classes[c].free == h,
            forall|c2: int|
                0 <= c2 < CLASSES && c2 != c ==> #[trigger] self.classes[c2] == s0.classes[c2],
            self.free@ == s0.free@.update(c, seq![h@.addr] + s0.free@[c]),
            self.in_use@ == s0.in_use@.remove(h@.addr),
            self.free_perms@ == s0.free_perms@.insert(h@.addr, self.free_perms@[h@.addr]),
            self.free_perms@[h@.addr].wf(h, s0.cap(h@.addr), s0.prefixed()),
            self.free_perms@[h@.addr].meta.is_init(),
            self.free_perms@[h@.addr].meta.value() == FREE_META,
            self.free_perms@[h@.addr].slots[0].is_init(),
            self.free_perms@[h@.addr].slots[0].value().ptr_spec()@.addr == s0.classes[c].free@.addr,
            self.free_perms@[h@.addr].slots[0].value().ptr_spec()@.provenance
                == s0.classes[c].free@.provenance,
        ensures
            self.wf(),
            self.in_use() == s0.in_use().remove(h@.addr),
            forall|b: usize| #[trigger] self.in_use().contains(b) ==> self.same_block(s0, b),
    {
        let hs = h@.addr;
        let old_list = s0.free@[c];
        let new_list = seq![hs] + old_list;
        assert forall|c2: int| 0 <= c2 < CLASSES implies #[trigger] self.classes[c2].page
            == s0.classes[c2].page && self.classes[c2].cursor == s0.classes[c2].cursor by {
            if c2 != c {
                assert(self.classes[c2] == s0.classes[c2]);
            }
        }
        self.lemma_layout_frame(s0);
        self.lemma_sum_frame(&s0, self.list@);
        reveal(Pages::list_wf);
        reveal(Pages::classes_wf);
        reveal(Pages::free_wf);
        reveal(Pages::carved_wf);
        assert(s0.class_wf(c));
        assert(!s0.free_perms@.contains_key(hs));
        assert(!old_list.contains(hs)) by {
            if old_list.contains(hs) {
                let k = choose|k: int| 0 <= k < old_list.len() && old_list[k] == hs;
                assert(s0.free_entry(c, k));
            }
        }
        assert forall|c2: int| 0 <= c2 < CLASSES implies #[trigger] self.class_wf(c2) by {
            assert(s0.class_wf(c2));
        }
        assert forall|c2: int, i: int|
            0 <= c2 < CLASSES && 0 <= i < self.free@[c2].len() implies #[trigger] self.free_entry(
            c2,
            i,
        ) by {
            if c2 == c {
                if i == 0 {
                    if old_list.len() > 0 {
                        assert(new_list[1] == old_list[0]);
                        assert(s0.classes[c].free == s0.block_ptr(old_list[0]));
                    }
                } else {
                    assert(s0.free_entry(c, i - 1));
                    assert(new_list[i] == old_list[i - 1]);
                    if i + 1 < new_list.len() {
                        assert(new_list[i + 1] == old_list[i]);
                    }
                }
            } else {
                assert(s0.free_entry(c2, i));
            }
        }
        assert forall|c2: int| 0 <= c2 < CLASSES implies (
        #[trigger] self.free@[c2]).no_duplicates() by {
            assert(s0.free@[c2].no_duplicates());
            if c2 == c {
                assert forall|i: int, j: int| 0 <= i < j < new_list.len() implies new_list[i]
                    != new_list[j] by {
                    if i == 0 {
                        assert(new_list[j] == old_list[j - 1]);
                    } else {
                        assert(new_list[i] == old_list[i - 1] && new_list[j] == old_list[j - 1]);
                    }
                }
            }
        }
        assert forall|h2: usize| #[trigger]
            self.free_perms@.contains_key(h2) implies self.on_free_list(h2) by {
            if h2 == hs {
                assert(new_list[0] == hs);
            } else {
                assert(s0.on_free_list(h2));
                let c2 = s0.hdr(page_base(h2)).class as int;
                if c2 == c {
                    let k = choose|k: int| 0 <= k < old_list.len() && old_list[k] == h2;
                    assert(new_list[k + 1] == h2);
                }
            }
        }
        assert(self.free_wf());
        assert(self.carved_wf());
        assert(self.classes_wf());
        assert(self.list_wf());
        assert(self.pages_wf());
        assert(self.in_use() =~= s0.in_use().remove(hs));
        assert forall|b: usize| #[trigger] self.in_use().contains(b) implies self.same_block(
            s0,
            b,
        ) by {
            assert(s0.carved@.contains(b));
        }
    }

    /// 大きい物体のページを OS に返す。
    #[cold]
    fn release_large(&mut self, h: *mut Header, Tracked(perm): Tracked<BlockPerm>)
        requires
            old(self).wf(),
            old(self).in_use().contains(h@.addr),
            h == old(self).block_ptr(h@.addr),
            perm.wf(h, old(self).cap(h@.addr), old(self).prefixed()),
            old(self).large(h@.addr),
        ensures
            final(self).wf(),
            final(self).prefixed() == old(self).prefixed(),
            final(self).in_use() == old(self).in_use().remove(h@.addr),
            forall|b: usize| #[trigger]
                final(self).in_use().contains(b) ==> final(self).same_block(*old(self), b),
    {
        let ghost s0 = *self;
        let ghost hs = h@.addr;
        proof {
            reveal(Pages::pages_wf);
            reveal(Pages::list_wf);
            reveal(Pages::carved_wf);
            self.lemma_block(hs);
        }
        let page = page_of(h);
        let hdr = self.header_of(page);
        let index = hdr.index;
        let bytes = hdr.bytes;
        let ghost a = page@.addr;
        proof {
            assert(self.list@[index as int]@.addr == a);
            self.lemma_sum_swap_remove(self.list@, index as int);
            self.lemma_sum_nonneg(self.list@.update(index as int, self.list@.last()).drop_last());
        }
        self.list.swap_remove(index);
        let ghost s1 = *self;
        if index < self.list.len() {
            let moved = self.list[index];
            proof {
                let last = s0.list@.len() - 1;
                assert(moved == s0.list@[last]);
                assert(s0.hdr(moved@.addr).index == last);
                assert(moved@.addr != a);
            }
            let mut mh = self.header_of(moved);
            mh.index = index;
            let tracked mut mp = self.pages.borrow_mut().tracked_remove(moved@.addr);
            ptr_mut_write(moved, Tracked(&mut mp.header), mh);
            proof {
                self.pages.borrow_mut().tracked_insert(moved@.addr, mp);
            }
        }
        self.bytes -= bytes;
        let tracked pp = self.pages.borrow_mut().tracked_remove(a);
        let tracked PagePerm { header: mut header, dealloc, tail } = pp;
        let ghost cap = s0.cap(hs);
        proof {
            header.leak_contents();
        }
        let tracked head = header.into_raw();
        let tracked block = perm.into_raw(h, cap, s0.prefixed());
        let tracked raw = head.join(block).join(tail);
        proof {
            assert(raw.dom() =~= Set::range(a as int, a + bytes));
        }
        dealloc_page(page as *mut u8, bytes, Tracked(raw), Tracked(dealloc));
        refund_pages(&self.control, bytes);
        proof {
            self.count = Ghost(self.count@.remove(a));
            self.carved = Ghost(self.carved@.remove(hs));
            self.index = Ghost(self.index@.remove(hs));
            self.in_use = Ghost(self.in_use@.remove(hs));
            self.lemma_release(s0, a, hs, index as int);
        }
    }

    /// 和は負にならない。
    proof fn lemma_sum_nonneg(&self, list: Seq<*mut PageHeader>)
        ensures
            self.sum_bytes(list) >= 0,
        decreases list.len(),
    {
        if list.len() > 0 {
            self.lemma_sum_nonneg(list.drop_last());
        }
    }

    proof fn lemma_release(&self, s0: Pages, a: usize, hs: usize, i: int)
        requires
            s0.wf(),
            s0.in_use().contains(hs),
            page_base(hs) == a,
            s0.pages@.contains_key(a),
            s0.hdr(a).class == LARGE,
            0 <= i < s0.list@.len(),
            s0.list@[i]@.addr == a,
            self.list@ == s0.list@.update(i, s0.list@.last()).drop_last(),
            self.pages@.dom() == s0.pages@.dom().remove(a),
            i < self.list@.len() ==> ({
                let m = s0.list@.last()@.addr;
                &&& self.hdr(m) == (PageHeader { index: i as usize, ..s0.hdr(m) })
                &&& self.pages@[m].header.ptr() == s0.pages@[m].header.ptr()
                &&& self.pages@[m].header.is_init()
                &&& self.pages@[m].dealloc == s0.pages@[m].dealloc
                &&& self.pages@[m].tail == s0.pages@[m].tail
                &&& forall|x: usize| x != a && x != m ==> #[trigger] self.pages@[x] == s0.pages@[x]
            }),
            i == self.list@.len() ==> forall|x: usize|
                x != a ==> #[trigger] self.pages@[x] == s0.pages@[x],
            self.bytes == s0.bytes - s0.hdr(a).bytes,
            self.count@ == s0.count@.remove(a),
            self.carved@ == s0.carved@.remove(hs),
            self.index@ == s0.index@.remove(hs),
            self.in_use@ == s0.in_use@.remove(hs),
            self.free_perms == s0.free_perms,
            self.free == s0.free,
            self.prefix_words == s0.prefix_words,
            self.classes == s0.classes,
        ensures
            self.wf(),
            forall|b: usize| #[trigger] self.in_use().contains(b) ==> self.same_block(s0, b),
    {
        reveal(Pages::list_wf);
        reveal(Pages::pages_wf);
        reveal(Pages::classes_wf);
        reveal(Pages::free_wf);
        reveal(Pages::carved_wf);
        let n = s0.list@.len();
        let m = s0.list@.last()@.addr;
        assert(s0.page_wf(a));
        // ほかのページの頭は、移したページの番号のほかは変わらない
        assert forall|x: usize| x != a && s0.pages@.contains_key(x) implies #[trigger] self.hdr(
            x,
        ).class == s0.hdr(x).class && self.hdr(x).block_words == s0.hdr(x).block_words && self.hdr(
            x,
        ).bytes == s0.hdr(x).bytes && self.hdr(x).limit == s0.hdr(x).limit && self.page_provenance(
            x,
        ) == s0.page_provenance(x) && self.pages@[x].header.ptr() == s0.pages@[x].header.ptr()
            && self.pages@[x].header.is_init() && self.pages@[x].dealloc == s0.pages@[x].dealloc
            && self.pages@[x].tail == s0.pages@[x].tail && self.limit_of(x) == s0.limit_of(x) by {
            assert(s0.page_wf(x));
            let cx = s0.hdr(x).class;
            if (cx as int) < CLASSES {
                assert(s0.class_wf(cx as int));
            }
        }
        // 大きい物体のページの切り出したブロックは hs だけである
        assert forall|h2: usize| #[trigger] s0.carved@.contains(h2) && page_base(h2) == a implies h2
            == hs by {
            assert(s0.index@[h2] < 1);
            assert(s0.carved@.contains(hs));
            assert(s0.index@[hs] < 1);
            assert(0 * s0.block_bytes_of(a) == 0);
        }
        assert forall|x: usize| #[trigger] self.pages@.contains_key(x) implies self.page_wf(x) by {
            assert(s0.page_wf(x));
        }
        assert(self.count@.dom() =~= self.pages@.dom());
        assert forall|c2: int| 0 <= c2 < CLASSES implies #[trigger] self.class_wf(c2) by {
            assert(s0.class_wf(c2));
            if s0.free@[c2].len() > 0 {
                let f = s0.free@[c2][0];
                assert(s0.free_entry(c2, 0));
                assert(page_base(f) != a);
            }
        }
        assert forall|h2: usize| #[trigger] self.carved@.contains(h2) implies page_base(h2) != a
            && self.block_ptr(h2) == s0.block_ptr(h2) && self.cap(h2) == s0.cap(h2)
            && self.block_words_spec(h2) == s0.block_words_spec(h2) && self.large(h2) == s0.large(
            h2,
        ) && self.block_bytes_spec(h2) == s0.block_bytes_spec(h2) by {
            assert(s0.carved@.contains(h2));
            assert(s0.pages@.contains_key(page_base(h2)));
        }
        assert forall|c2: int, j: int|
            0 <= c2 < CLASSES && 0 <= j < self.free@[c2].len() implies #[trigger] self.free_entry(
            c2,
            j,
        ) by {
            assert(s0.free_entry(c2, j));
            let f = s0.free@[c2][j];
            assert(page_base(f) != a);
            assert(f != hs);
            if j + 1 < self.free@[c2].len() {
                assert(s0.free_entry(c2, j + 1));
            }
        }
        assert forall|h2: usize| #[trigger]
            self.free_perms@.contains_key(h2) implies self.on_free_list(h2) by {
            assert(s0.on_free_list(h2));
            assert(s0.carved@.contains(h2));
            assert(h2 != hs);
        }
        assert forall|x: usize, k: int|
            self.pages@.contains_key(x) && 0 <= k < self.count@[x] implies self.carved@.contains(
            #[trigger] self.block_at(x, k),
        ) by {
            assert(self.block_at(x, k) == s0.block_at(x, k));
            assert(s0.carved@.contains(s0.block_at(x, k)));
            s0.lemma_block_at(x, k);
        }
        assert forall|h2: usize| #[trigger] self.carved@.contains(h2) implies ({
            let x = page_base(h2);
            &&& self.pages@.contains_key(x)
            &&& self.index@.contains_key(h2)
            &&& self.index@[h2] < self.count@[x]
            &&& h2 == self.block_at(x, self.index@[h2] as int)
        }) by {
            assert(s0.carved@.contains(h2));
            assert(self.block_at(page_base(h2), s0.index@[h2] as int) == s0.block_at(
                page_base(h2),
                s0.index@[h2] as int,
            ));
        }
        assert(self.carved_wf());
        assert(self.free_wf());
        assert(self.classes_wf());
        assert(self.pages_wf());
        // 一覧
        assert forall|j: int| 0 <= j < self.list@.len() implies self.pages@.contains_key(
            #[trigger] self.list@[j]@.addr,
        ) by {
            if j == i {
                assert(self.list@[j] == s0.list@[n - 1]);
            } else {
                assert(self.list@[j] == s0.list@[j]);
            }
        }
        assert forall|x: usize| #[trigger] self.pages@.contains_key(x) implies 0 <= self.hdr(
            x,
        ).index < self.list@.len() && self.list@[self.hdr(x).index as int]@.addr == x by {
            assert(s0.pages@.contains_key(x));
            let k = s0.hdr(x).index as int;
            if x == m && i < self.list@.len() {
                assert(self.list@[i] == s0.list@[n - 1]);
            } else {
                assert(k != n - 1 || x == m);
                assert(k != i);
                assert(self.list@[k] == s0.list@[k]);
            }
        }
        assert forall|j: int| 0 <= j < self.list@.len() implies self.hdr(
            #[trigger] self.list@[j]@.addr,
        ).index == j by {
            if j == i {
                assert(self.list@[j] == s0.list@[n - 1]);
            } else {
                assert(self.list@[j] == s0.list@[j]);
                assert(s0.list@[j]@.addr != m);
            }
        }
        assert forall|j: int| 0 <= j < self.list@.len() implies #[trigger] self.list@[j]
            == self.pages@[self.list@[j]@.addr].header.ptr() by {
            if j == i {
                assert(self.list@[j] == s0.list@[n - 1]);
            } else {
                assert(self.list@[j] == s0.list@[j]);
            }
        }
        assert(self.list_wf());
        // バイト数
        s0.lemma_sum_swap_remove(s0.list@, i);
        assert forall|j: int| 0 <= j < self.list@.len() implies self.hdr(
            #[trigger] self.list@[j]@.addr,
        ).bytes == s0.hdr(self.list@[j]@.addr).bytes by {
            if j == i {
                assert(self.list@[j] == s0.list@[n - 1]);
            } else {
                assert(self.list@[j] == s0.list@[j]);
            }
        }
        self.lemma_sum_frame(&s0, self.list@);
        assert(self.wf());
        assert forall|b: usize| #[trigger] self.in_use().contains(b) implies self.same_block(
            s0,
            b,
        ) by {
            assert(s0.carved@.contains(b));
        }
    }

    /// ページの頭 `a` の権限を持つ。
    /// 持っているページ `a` の頭の値。
    pub(crate) closed spec fn page_header(&self, a: usize) -> PageHeader {
        self.hdr(a)
    }

    pub(crate) closed spec fn has_page(&self, a: usize) -> bool {
        self.pages@.contains_key(a)
    }

    /// 切り出したブロックは、持っているページにある。
    pub(crate) proof fn lemma_carved_page(&self, h: usize)
        requires
            self.wf(),
            self.carved_block(h),
        ensures
            self.has_page(page_base(h)),
    {
        reveal(Pages::carved_wf);
    }

    /// 持っているページ `a` は、凍結した塊の鎖 `r` のページでない。ページの頭の権限は1つしかないからである。
    pub(crate) proof fn lemma_page_apart(
        tracked &mut self,
        a: usize,
        tracked r: &RegionPerms,
        prefixed: bool,
    )
        requires
            old(self).wf(),
            old(self).has_page(a),
            r.wf(prefixed),
        ensures
            *final(self) == *old(self),
            !r.has_page(a),
    {
        reveal(Pages::pages_wf);
        assert(self.page_wf(a));
        let tracked mut page = self.pages.borrow_mut().tracked_remove(a);
        r.lemma_apart(&mut page.header, prefixed);
        self.pages.borrow_mut().tracked_insert(a, page);
        assert(self.pages@ =~= old(self).pages@);
    }

    /// 持っているページは、どれも凍結した塊の鎖 `r` のページでない。
    pub(crate) proof fn lemma_pages_apart(
        tracked &mut self,
        tracked r: &RegionPerms,
        prefixed: bool,
    )
        requires
            old(self).wf(),
            r.wf(prefixed),
        ensures
            *final(self) == *old(self),
            forall|a: usize| #[trigger] final(self).has_page(a) ==> !r.has_page(a),
    {
        reveal(Pages::pages_wf);
        let ghost keys = self.pages@.dom();
        assert forall|a: usize| #[trigger]
            keys.contains(a) implies self.pages@[a].header.ptr()@.addr == a by {
            assert(self.page_wf(a));
        }
        pages_apart(self.pages.borrow_mut(), keys, r, prefixed);
    }

    /// 渡したブロックは、切り出したブロックである。
    pub(crate) proof fn lemma_in_use_carved(&self, h: usize)
        requires
            self.wf(),
            self.in_use().contains(h),
        ensures
            self.carved_block(h),
    {
        reveal(Pages::carved_wf);
    }

    /// 切り出したブロック `h` のヘッダの `meta` を読む。渡したブロックなら持ち主の権限 `held` で読み、渡していない
    /// ブロックなら空きリストにあるので、確保器の権限で読む。値は空いた印である。
    #[inline]
    pub(crate) fn read_meta(
        &self,
        h: *mut Header,
        Tracked(held): Tracked<Option<&BlockPerm>>,
    ) -> (r: u32)
        requires
            self.wf(),
            self.carved_block(h@.addr),
            h == self.block_ptr(h@.addr),
            held is Some <==> self.in_use().contains(h@.addr),
            held matches Some(p) ==> p.meta.ptr() == field_ptr::<u32>(h, 4) && p.meta.is_init(),
        ensures
            held matches Some(p) ==> r == p.meta.value(),
            held is None ==> r == FREE_META,
    {
        proof {
            self.lemma_block_bounds(h@.addr);
            reveal(Pages::carved_wf);
            reveal(Pages::free_wf);
            if held is None {
                assert(self.carved@.contains(h@.addr));
                assert(self.on_free_list(h@.addr));
                let c = self.hdr(page_base(h@.addr)).class as int;
                let i = self.free@[c].index_of(h@.addr);
                assert(self.free_entry(c, i));
            }
        }
        let tracked p = match held {
            Some(p) => p,
            None => self.free_perms.borrow().tracked_borrow(h@.addr),
        };
        *ptr_ref(meta_ptr(h), Tracked(&p.meta))
    }

    /// 持っているページの数。
    pub(crate) closed spec fn page_count(&self) -> nat {
        self.list@.len()
    }

    pub(crate) fn pages_len(&self) -> (r: usize)
        ensures
            r == self.page_count(),
    {
        self.list.len()
    }

    /// 切り出したすべてのブロックのヘッダの位置。空いたブロックも含むので、見分けるのは呼ぶ側である。
    pub(crate) fn blocks(&self) -> (r: Vec<*mut Header>)
        requires
            self.wf(),
        ensures
            forall|i: int|
                0 <= i < r@.len() ==> self.carved_block(#[trigger] r@[i]@.addr) && r@[i]
                    == self.block_ptr(r@[i]@.addr),
            forall|h: usize| #[trigger]
                self.carved_block(h) ==> exists|i: int|
                    #![trigger r@[i]]
                    0 <= i < r@.len() && r@[i]@.addr == h,
    {
        self.blocks_in(0, self.list.len())
    }

    /// 一覧の `lo` 番から `hi` 番の手前までのページの、切り出したブロックのヘッダの位置。各級の今のページではヒープの
    /// 切り出しの位置を読み、まだ切り出していないメモリをブロックとして読まない。ページごとに読めば、ヒープ全体の
    /// 一覧を作らずにすべてのブロックを見られる。
    pub(crate) fn blocks_in(&self, lo: usize, hi: usize) -> (r: Vec<*mut Header>)
        requires
            self.wf(),
            lo <= hi <= self.page_count(),
        ensures
            forall|i: int|
                0 <= i < r@.len() ==> self.carved_block(#[trigger] r@[i]@.addr) && r@[i]
                    == self.block_ptr(r@[i]@.addr),
            lo == 0 && hi == self.page_count() ==> forall|h: usize| #[trigger]
                self.carved_block(h) ==> exists|i: int|
                    #![trigger r@[i]]
                    0 <= i < r@.len() && r@[i]@.addr == h,
    {
        proof {
            reveal(Pages::list_wf);
            reveal(Pages::pages_wf);
            reveal(Pages::carved_wf);
            reveal(Pages::classes_wf);
            assert forall|j: int| 0 <= j < self.list@.len() implies #[trigger] self.listed(j) by {
                assert(self.pages@.contains_key(self.list@[j]@.addr));
            }
            assert forall|a: usize, k: int|
                self.pages@.contains_key(a) && 0 <= k
                    < self.count@[a] implies self.carved@.contains(
                #[trigger] self.block_at(a, k),
            ) by {}
            assert forall|h: usize| #[trigger] self.carved_block(h) implies self.located(h) by {
                assert(self.pages@.contains_key(page_base(h)));
            }
        }
        let mut out: Vec<*mut Header> = Vec::new();
        let ghost mut pos: Map<usize, int> = Map::empty();
        let mut pi: usize = lo;
        while pi < hi
            invariant
                self.wf(),
                forall|j: int| 0 <= j < self.list@.len() ==> #[trigger] self.listed(j),
                forall|a: usize, k: int|
                    self.pages@.contains_key(a) && 0 <= k < self.count@[a]
                        ==> self.carved@.contains(#[trigger] self.block_at(a, k)),
                forall|c: int| 0 <= c < CLASSES ==> #[trigger] self.class_wf(c),
                lo <= pi <= hi,
                hi <= self.list@.len(),
                forall|i: int|
                    0 <= i < out@.len() ==> self.carved_block(#[trigger] out@[i]@.addr) && out@[i]
                        == self.block_ptr(out@[i]@.addr),
                forall|j: int, k: int|
                    lo <= j < pi && 0 <= k < self.count@[self.list@[j]@.addr] ==> pos.contains_key(
                        #[trigger] self.block_at(self.list@[j]@.addr, k),
                    ) && 0 <= pos[self.block_at(self.list@[j]@.addr, k)] < out@.len()
                        && out@[pos[self.block_at(self.list@[j]@.addr, k)]]@.addr == self.block_at(
                        self.list@[j]@.addr,
                        k,
                    ),
            decreases hi - pi,
        {
            let page = self.list[pi];
            let ghost a = page@.addr;
            proof {
                assert(self.listed(pi as int));
            }
            let hdr = self.header_of(page);
            let class = hdr.class as usize;
            let limit = if class < CLASSES && self.classes[class].page.addr() == page.addr() {
                proof {
                    assert(self.class_wf(class as int));
                }
                self.classes[class].cursor.addr() - page.addr()
            } else {
                hdr.limit
            };
            let block_bytes = hdr.block_words as usize * WORD;
            let mut offset = FIRST_BLOCK;
            let ghost mut k: int = 0;
            let ghost bb = block_bytes as int;
            let ghost n = self.count@[a] as int;
            proof {
                assert(limit == self.limit_of(a));
                assert(limit == FIRST_BLOCK + n * bb);
                assert(0 * bb == 0);
                assert(limit <= self.hdr(a).bytes) by (nonlinear_arith)
                    requires
                        limit == FIRST_BLOCK + n * bb,
                        (self.hdr(a).class as int) < CLASSES ==> limit <= PAGE_BYTES && self.hdr(
                            a,
                        ).bytes == PAGE_BYTES,
                        self.hdr(a).class == LARGE ==> n == 1 && self.hdr(a).bytes == FIRST_BLOCK
                            + bb,
                        (self.hdr(a).class as int) < CLASSES || self.hdr(a).class == LARGE,
                ;
            }
            while offset + block_bytes <= limit
                invariant
                    self.wf(),
                    forall|j: int| 0 <= j < self.list@.len() ==> #[trigger] self.listed(j),
                    forall|a: usize, k: int|
                        self.pages@.contains_key(a) && 0 <= k < self.count@[a]
                            ==> self.carved@.contains(#[trigger] self.block_at(a, k)),
                    forall|c: int| 0 <= c < CLASSES ==> #[trigger] self.class_wf(c),
                    pi < hi,
                    lo <= pi,
                    hi <= self.list@.len(),
                    page == self.list@[pi as int],
                    a == page@.addr,
                    self.pages@.contains_key(a),
                    self.page_wf(a),
                    bb == self.block_bytes_of(a),
                    bb == block_bytes,
                    bb > 0,
                    limit == FIRST_BLOCK + n * bb,
                    limit <= MAX_PAGE_BYTES,
                    bb <= MAX_PAGE_BYTES,
                    offset <= limit,
                    n == self.count@[a],
                    0 <= k <= n,
                    offset == FIRST_BLOCK + k * bb,
                    forall|i: int|
                        0 <= i < out@.len() ==> self.carved_block(#[trigger] out@[i]@.addr)
                            && out@[i] == self.block_ptr(out@[i]@.addr),
                    forall|j: int, kk: int|
                        lo <= j < pi && 0 <= kk < self.count@[self.list@[j]@.addr]
                            ==> pos.contains_key(#[trigger] self.block_at(self.list@[j]@.addr, kk))
                            && 0 <= pos[self.block_at(self.list@[j]@.addr, kk)] < out@.len()
                            && out@[pos[self.block_at(self.list@[j]@.addr, kk)]]@.addr
                            == self.block_at(self.list@[j]@.addr, kk),
                    forall|kk: int|
                        0 <= kk < k ==> pos.contains_key(#[trigger] self.block_at(a, kk)) && 0
                            <= pos[self.block_at(a, kk)] < out@.len() && out@[pos[self.block_at(
                            a,
                            kk,
                        )]]@.addr == self.block_at(a, kk),
                decreases n - k,
            {
                proof {
                    assert(k < n) by (nonlinear_arith)
                        requires
                            FIRST_BLOCK + k * bb + bb <= FIRST_BLOCK + n * bb,
                            bb > 0,
                    ;
                    assert(k * bb + bb == (k + 1) * bb) by (nonlinear_arith);
                    self.lemma_block_at(a, k);
                    assert(self.carved@.contains(self.block_at(a, k)));
                    assert((k + 1) * bb <= n * bb) by (nonlinear_arith)
                        requires
                            k + 1 <= n,
                            bb > 0,
                    ;
                }
                let h = (page as *mut Header).with_addr(
                    page.addr() + offset + self.prefix_words * WORD,
                );
                proof {
                    assert(h@.addr == self.block_at(a, k));
                    assert(self.listed(pi as int));
                    assert(page == self.pages@[a].header.ptr());
                    assert(self.page_provenance(a) == page@.provenance);
                }
                let ghost before = out@;
                out.push(h);
                proof {
                    pos = pos.insert(h@.addr, before.len() as int);
                    assert forall|j: int, kk: int|
                        lo <= j < pi && 0 <= kk
                            < self.count@[self.list@[j]@.addr] implies pos.contains_key(
                        #[trigger] self.block_at(self.list@[j]@.addr, kk),
                    ) && 0 <= pos[self.block_at(self.list@[j]@.addr, kk)] < out@.len()
                        && out@[pos[self.block_at(self.list@[j]@.addr, kk)]]@.addr == self.block_at(
                        self.list@[j]@.addr,
                        kk,
                    ) by {
                        let b = self.block_at(self.list@[j]@.addr, kk);
                        if b == h@.addr {
                            assert(out@[before.len() as int] == h);
                        } else {
                            assert(out@[pos[b]] == before[pos[b]]);
                        }
                    }
                    assert forall|kk: int| 0 <= kk < k + 1 implies pos.contains_key(
                        #[trigger] self.block_at(a, kk),
                    ) && 0 <= pos[self.block_at(a, kk)] < out@.len() && out@[pos[self.block_at(
                        a,
                        kk,
                    )]]@.addr == self.block_at(a, kk) by {
                        let b = self.block_at(a, kk);
                        if b != h@.addr {
                            assert(out@[pos[b]] == before[pos[b]]);
                        }
                    }
                    k = k + 1;
                }
                offset += block_bytes;
            }
            proof {
                assert(k == n) by (nonlinear_arith)
                    requires
                        FIRST_BLOCK + k * bb + bb > FIRST_BLOCK + n * bb,
                        k <= n,
                        bb > 0,
                ;
            }
            pi += 1;
        }
        proof {
            assert forall|h: usize|
                #![trigger self.carved_block(h)]
                lo == 0 && hi == self.list@.len() && self.carved_block(h) implies exists|i: int|
                #![trigger out@[i]]
                0 <= i < out@.len() && out@[i]@.addr == h by {
                assert(self.located(h));
                let a = page_base(h);
                let j = self.hdr(a).index as int;
                let kk = self.index@[h] as int;
                assert(self.list@[j]@.addr == a);
                assert(h == self.block_at(a, kk));
                assert(out@[pos[h]]@.addr == h);
            }
        }
        out
    }
}

// OS との境界 (信頼の一覧の T1)。vstd の `allocate` は確保に失敗すると `process::abort` を呼ぶので使わず、
// `handle_alloc_error` を呼ぶ形で包む。約束は vstd の `allocate` と `deallocate` と同じ形にする。
#[verifier::external_body]
fn alloc_page(bytes: usize) -> (r: (*mut u8, Tracked<PointsToRaw>, Tracked<Dealloc>))
    requires
        0 < bytes <= MAX_PAGE_BYTES,
    ensures
        r.1@.is_range(r.0@.addr as int, bytes as int),
        // Rust の確保した領域はアドレス空間の端をまたがないので、終わりの番地も usize に収まる
        r.0@.addr + bytes <= usize::MAX,
        r.0@.addr % PAGE_BYTES == 0,
        r.0@.addr > 0,
        r.2@.addr() == r.0@.addr,
        r.2@.size() == bytes,
        r.2@.align() == PAGE_BYTES,
        r.2@.provenance() == r.1@.provenance(),
        r.0@.provenance == r.1@.provenance(),
{
    let layout = unsafe { Layout::from_size_align_unchecked(bytes, PAGE_BYTES) };
    let p = unsafe { alloc(layout) };
    if p.is_null() {
        handle_alloc_error(layout)
    }
    (p, Tracked::assume_new(), Tracked::assume_new())
}

#[verifier::external_body]
fn dealloc_page(
    p: *mut u8,
    bytes: usize,
    Tracked(raw): Tracked<PointsToRaw>,
    Tracked(page): Tracked<Dealloc>,
)
    requires
        page.addr() == p@.addr,
        page.size() == bytes,
        page.align() == PAGE_BYTES,
        page.provenance() == raw.provenance(),
        raw.is_range(p@.addr as int, bytes as int),
        p@.provenance == page.provenance(),
{
    unsafe { dealloc(p, Layout::from_size_align_unchecked(bytes, PAGE_BYTES)) }
}

// セッションのページの数の加減 (信頼の一覧の T1)。値について何も約束しないので、検証には何も持ち込まない
#[verifier::external_body]
fn charge_pages(control: &Control, bytes: usize) {
    control.charge(bytes)
}

#[verifier::external_body]
fn refund_pages(control: &Control, bytes: usize) {
    control.refund(bytes)
}

/// `bytes` を確保すると確保量の上限を超えるか (T1)。値について何も約束しないので、どちらを返しても証明は崩れない。
#[verifier::external_body]
pub(crate) fn over_budget(control: &Control, bytes: usize) -> bool {
    control.over_budget(bytes)
}

// 止める関数 (信頼の一覧の T5)。どれも状態を変える前に呼ぶ (T7)
#[verifier::external_body]
#[cold]
pub(crate) fn too_large() -> ! {
    panic!("{}", TOO_LARGE)
}

#[verifier::external_body]
#[cold]
fn too_many_words() -> ! {
    panic!("an object of fewer than 2^32 words")
}

#[verifier::external_body]
#[cold]
fn bad_page_layout() -> ! {
    panic!("a page layout")
}

} // verus!

const _: () = assert!(FIRST_BLOCK == size_of::<PageHeader>().next_multiple_of(WORD));
const _: () = assert!(PAGE_BYTES == 1 << 16);
const _: () = assert!(MAX_PAGE_BYTES == isize::MAX as usize - (isize::MAX as usize % PAGE_BYTES));

impl Drop for Pages {
    fn drop(&mut self) {
        for page in self.list.drain(..) {
            let bytes = unsafe { (*page).bytes };
            unsafe {
                dealloc(
                    page.cast(),
                    Layout::from_size_align_unchecked(bytes, PAGE_BYTES),
                );
            }
            self.control.refund(bytes);
        }
        // `control` の欄はこの後で読まない
        release_control(unsafe { ManuallyDrop::take(&mut self.control) });
    }
}

/// ページの数を数える先の `Arc` を手放す。呼ぶ側に展開させない理由は、`Pages` の `control` の欄の注釈にある。
#[inline(never)]
fn release_control(control: Arc<Control>) {
    let _ = control;
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::meta::{self, Kind, Meta};

    fn init(h: *mut Header) {
        unsafe {
            meta::rc_ptr(h).write(1);
            meta::meta_ptr(h).write(Meta::new(0, 0, Kind::Data, false).bits());
        }
    }

    fn new_pages(prefix_words: usize) -> Pages {
        Pages::new(
            prefix_words,
            Arc::new(Control::new(crate::Limits::default())),
        )
    }

    fn blocks(pages: &Pages) -> usize {
        pages.blocks().len()
    }

    fn alloc(pages: &mut Pages, words: usize) -> *mut Header {
        pages.alloc(words).0
    }

    fn free(pages: &mut Pages, h: *mut Header) {
        pages.free(h, Tracked::assume_new())
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
        let mut pages = new_pages(0);
        let a = alloc(&mut pages, 3);
        let b = alloc(&mut pages, 3);
        assert_eq!(a.addr() % WORD, 0);
        assert_eq!(b.addr() - a.addr(), 3 * WORD);
        assert_eq!(pages.block_words(a), 3);
        assert_eq!(pages.bytes(), PAGE_BYTES);
    }

    #[test]
    fn a_freed_block_is_marked_free_and_reused_first() {
        let mut pages = new_pages(0);
        let a = alloc(&mut pages, 2);
        init(a);
        init(alloc(&mut pages, 2));
        free(&mut pages, a);
        assert_eq!(
            Meta::from_bits(unsafe { meta::meta_ptr(a).read() }).kind(),
            Kind::Free
        );
        assert_eq!(alloc(&mut pages, 2), a);
    }

    #[test]
    fn a_new_page_writes_back_where_the_old_one_stopped() {
        let mut pages = new_pages(0);
        let per_page = (PAGE_BYTES - FIRST_BLOCK) / (16 * WORD);
        for _ in 0..per_page + 3 {
            init(alloc(&mut pages, 16));
        }
        assert_eq!(pages.list.len(), 2);
        assert_eq!(blocks(&pages), per_page + 3);
    }

    #[test]
    fn a_large_object_gets_its_own_page_and_is_returned_at_once() {
        let mut pages = new_pages(0);
        init(alloc(&mut pages, 2));
        let [a, b, c] = [200, 300, 400].map(|words| {
            let h = alloc(&mut pages, words);
            init(h);
            h
        });
        assert_eq!(pages.list.len(), 4);
        let before = pages.bytes();
        free(&mut pages, a);
        assert_eq!(pages.list.len(), 3);
        assert_eq!(pages.bytes(), before - (FIRST_BLOCK + 200 * WORD));
        // a の添字に移った c も続けて返せる
        free(&mut pages, c);
        free(&mut pages, b);
        assert_eq!(pages.list.len(), 1);
        assert_eq!(blocks(&pages), 1);
    }

    #[test]
    fn the_prefix_shifts_the_header_and_the_class() {
        let mut pages = new_pages(1);
        // 前置きを足して 17 語なので、20 語の級になる
        let a = alloc(&mut pages, 16);
        assert_eq!(pages.block_words(a), 20);
        let offset = a.addr() - page_of(a).addr() - FIRST_BLOCK;
        assert_eq!(offset % (20 * WORD), WORD);
        assert!(pages.fits(a, 16));
        assert!(pages.fits(a, 19));
        assert!(!pages.fits(a, 15));
        assert!(!pages.fits(a, 20));
    }

    // 前置きを足す和が桁あふれするのは、検査モードの `con_at` に巨大な語の数が来たときだけである。`con_at` は安全の条件で
    // 級を約束させるので、ここでは `fits` を直に呼ぶ
    #[test]
    fn a_wrapping_size_panics_in_the_token_check() {
        let mut pages = new_pages(1);
        let a = alloc(&mut pages, 2);
        let payload =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| pages.fits(a, usize::MAX)))
                .expect_err("a wrapping size must panic");
        let message = payload
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| payload.downcast_ref::<String>().map(String::as_str));
        assert_eq!(message, Some(TOO_LARGE));
    }
}
