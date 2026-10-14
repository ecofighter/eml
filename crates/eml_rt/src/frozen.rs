//! 凍結した領域 (docs/spec/object-model.md の「不死の物体の数え方」)。不死の定数を、どのヒープにも属さない読み出し
//! 専用の塊に置く。塊は親の塊への `Arc` を持ち、鎖になる。凍結した後はどの操作も書き込まないので、権限は vstd の
//! `Shared` で読むだけの形にして、付けたヒープに写す。

use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

use vstd::prelude::*;
use vstd::raw_ptr::PointsTo;
#[cfg(verus_keep_ghost)]
use vstd::shared::Shared;

use crate::alloc::{PageHeader, Pages};
#[cfg(verus_only)]
use crate::alloc::{page_base, region_cap, region_ptr};
#[cfg(verus_only)]
use crate::heap::check::UNCOUNTED;
#[cfg(verus_only)]
use crate::heap::inv::{
    p_code, p_count, p_extended, p_first, p_init, p_kind, p_meta, p_owner, p_scanned, p_word,
    string_wf,
};
use crate::meta::BlockPerm;
#[cfg(verus_only)]
use crate::meta::{HEADER_WORDS, Header, Kind, bits_wf};
use crate::mode::Mode;
#[cfg(verus_only)]
use crate::word::Word;

verus! {

/// 塊の権限。この塊の生きているブロックの権限と、そのページの頭の読む権限と、親の塊の権限の写しを持つ。
// 仕様だけで使う型。Verus の外のビルドでも型は残るので、使わないことの警告を止める
#[cfg_attr(not(verus_keep_ghost), allow(dead_code))]
pub(crate) tracked struct RegionPerms {
    pub(crate) tracked blocks: Map<usize, BlockPerm>,
    pub(crate) tracked headers: Map<usize, PointsTo<PageHeader>>,
    // vstd の `Shared` は Verus のビルドにだけある
    #[cfg(verus_keep_ghost)]
    pub(crate) tracked parent: Option<Shared<RegionPerms>>,
    /// 鎖の長さ。親をたどる再帰の尺度にする。
    pub(crate) ghost depth: nat,
}

impl RegionPerms {
    /// 親のない塊の権限。
    pub(crate) proof fn new(
        tracked blocks: Map<usize, BlockPerm>,
        tracked headers: Map<usize, PointsTo<PageHeader>>,
    ) -> (tracked r: RegionPerms)
        ensures
            r.blocks == blocks,
            r.headers == headers,
            r.parent is None,
            r.depth == 0,
    {
        RegionPerms { blocks, headers, parent: None, depth: 0 }
    }

    /// `h` は、この塊か祖先の塊のブロックである。
    pub(crate) open spec fn has(self, h: usize) -> bool
        decreases self.depth,
    {
        ||| self.blocks.dom().contains(h)
        ||| self.parent matches Some(p) && p@.depth < self.depth && p@.has(h)
    }

    /// ブロック `h` を持つ塊の権限。`has(h)` でなければ意味を持たない。
    pub(crate) open spec fn owner(self, h: usize) -> RegionPerms
        decreases self.depth,
    {
        match self.parent {
            Some(p) if !self.blocks.dom().contains(h) && p@.depth < self.depth => p@.owner(h),
            _ => self,
        }
    }

    pub(crate) open spec fn perm(self, h: usize) -> BlockPerm {
        self.owner(h).blocks[h]
    }

    pub(crate) open spec fn block(self, h: usize) -> *mut Header {
        region_ptr(self.owner(h).headers, h)
    }

    pub(crate) open spec fn cap(self, h: usize, prefixed: bool) -> nat {
        region_cap(self.owner(h).headers, h, prefixed)
    }

    /// 語 `w` は即値か、この塊か祖先の塊のブロックを、そのブロックの provenance で指す。
    pub(crate) open spec fn word_ok(self, w: Word) -> bool {
        w.is_ptr_spec() ==> {
            &&& self.has(w.bits_spec())
            &&& w.ptr_spec()@.provenance == self.block(w.bits_spec())@.provenance
        }
    }

    /// この塊のブロック `h` は、生存の rc 0 の物体で、形がヒープのブロックと同じ (`Core::block_shape`) である。検査
    /// モードの前置きは生存の状態で、数えない印を持つ (docs/spec/object-model.md の「不死の物体の数え方」)。
    pub(crate) open spec fn block_wf(self, h: usize, prefixed: bool) -> bool {
        let p = self.blocks[h];
        let ptr = region_ptr(self.headers, h);
        let cap = region_cap(self.headers, h, prefixed);
        &&& self.headers.contains_key(page_base(h))
        &&& ptr@.addr == h
        &&& h % 8 == 0
        &&& h >= 8
        &&& h + 8 * (HEADER_WORDS + cap) <= usize::MAX
        &&& p.wf(ptr, cap, prefixed)
        &&& p.rc.is_init()
        &&& p.rc.value() == 0
        &&& p.meta.is_init()
        &&& bits_wf(p_meta(p))
        &&& p_kind(p) != Kind::Free
        &&& p_extended(p) ==> cap >= 1 && p_init(p, 0)
        &&& p_first(p) + p_count(p) <= cap
        &&& p_kind(p) == Kind::BigInt ==> cap >= 1 && p_init(p, 0)
        &&& p_kind(p) == Kind::String ==> string_wf(p, cap)
        &&& p.prefix matches Some(q) ==> q.first.is_init() && q.state.is_init()
        &&& p.prefix is Some ==> p_code(p) == 1 && p_owner(p) == UNCOUNTED
        &&& forall|i: int| #[trigger] p_scanned(p, i) ==> p_init(p, i) && self.word_ok(p_word(p, i))
    }

    /// 塊とその祖先の不変条件。この塊のブロックは祖先のブロックと重ならない。
    pub(crate) open spec fn wf(self, prefixed: bool) -> bool
        decreases self.depth,
    {
        &&& forall|a: usize| #[trigger]
            self.headers.contains_key(a) ==> self.headers[a].ptr()@.addr == a
                && self.headers[a].is_init()
        &&& forall|h: usize| #[trigger] self.blocks.dom().contains(h) ==> self.block_wf(h, prefixed)
        &&& match self.parent {
            None => self.depth == 0,
            Some(p) => {
                &&& p@.depth + 1 == self.depth
                &&& p@.wf(prefixed)
                &&& forall|h: usize| #[trigger] self.blocks.dom().contains(h) ==> !p@.has(h)
            },
        }
    }
}

/// 凍結した塊。不死の定数のブロックを持つページと、親の塊への `Arc` を持つ (docs/spec/object-model.md の
/// 「不死の物体の数え方」)。
pub struct Frozen<M: Mode> {
    // `Drop` が返すページの一覧。凍結の後は `Drop` だけが読むので、コンパイラには読まない欄に見える
    #[allow(dead_code)]
    pages: Pages,
    parent: Option<Arc<Frozen<M>>>,
    #[cfg(verus_keep_ghost)]
    perms: Tracked<Shared<RegionPerms>>,
    _mode: PhantomData<M>,
}

impl<M: Mode> Frozen<M> {
    /// 塊の不変条件。権限は塊とその祖先の不変条件を満たし、権限の親は親の塊の権限である。
    #[verifier::type_invariant]
    pub closed spec fn inv(self) -> bool {
        &&& self.perms@@.wf(M::CHECKED)
        &&& match self.parent {
            None => self.perms@@.parent is None,
            Some(p) => self.perms@@.parent matches Some(s) && s@ == p.perms@@,
        }
    }

    /// `h` は、この塊か祖先の塊のブロックである。
    pub closed spec fn has(&self, h: usize) -> bool {
        self.perms@@.has(h)
    }

    /// 鎖の長さ。
    pub closed spec fn depth(&self) -> nat {
        self.perms@@.depth
    }

    /// 作り手から移したページとブロックの権限で、親のない塊を作る。
    pub(crate) fn new(pages: Pages, Tracked(perms): Tracked<RegionPerms>) -> (r: Frozen<M>)
        requires
            perms.wf(M::CHECKED),
            perms.parent is None,
        ensures
            r.inv(),
            r.depth() == 0,
            forall|h: usize| #[trigger] r.has(h) <==> perms.blocks.dom().contains(h),
    {
        #[cfg(verus_keep_ghost)]
        let tracked shared = Shared::new(perms);
        Frozen {
            pages,
            parent: None,
            #[cfg(verus_keep_ghost)]
            perms: Tracked(shared),
            _mode: PhantomData,
        }
    }
}

} // verus!

impl<M: Mode> fmt::Debug for Frozen<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Frozen").finish_non_exhaustive()
    }
}

// 親への `Arc` を、再帰せずに手放す。長い鎖の最後の `Arc` を落としてもスタックを溢れさせないためである。ページは
// `Pages` の `Drop` が返す (docs/spec/object-model.md の「`unsafe` の核」の T4)
impl<M: Mode> Drop for Frozen<M> {
    fn drop(&mut self) {
        let mut next = self.parent.take();
        while let Some(region) = next {
            next = Arc::into_inner(region).and_then(|mut frozen| frozen.parent.take());
        }
    }
}

// 凍結した後は、どの操作も塊のメモリに書き込まず、どのスレッドも読むだけである (docs/spec/object-model.md の
// 「`unsafe` の核」の T0 の改め)。ページの一覧の生のポインタは、最後の `Arc` を落とした `Drop` だけが使う
unsafe impl<M: Mode> Send for Frozen<M> {}
unsafe impl<M: Mode> Sync for Frozen<M> {}
