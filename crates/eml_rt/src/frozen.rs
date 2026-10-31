//! 凍結した領域 (docs/spec/object-model.md の「凍結した領域」)。不死の定数を、どのヒープにも属さない読み出し
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
use crate::alloc::{header_ok, header_sink, lemma_header_size, page_base, region_cap, region_ptr};
use crate::control::control_knows;
#[cfg(verus_only)]
use crate::heap::Env;
#[cfg(verus_only)]
use crate::heap::check::UNCOUNTED;
#[cfg(verus_only)]
use crate::heap::inv::{
    body_wf, p_code, p_count, p_extended, p_first, p_init, p_kind, p_meta, p_owner, p_scanned,
    p_word,
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
    /// 作り手の権限で塊の権限を作る。親は、作り手が付けていた塊の権限の写し (`env` の `region`) である。
    pub(crate) proof fn new(
        tracked blocks: Map<usize, BlockPerm>,
        tracked headers: Map<usize, PointsTo<PageHeader>>,
        tracked env: Env,
    ) -> (tracked r: RegionPerms)
        ensures
            r.blocks == blocks,
            r.headers == headers,
            r.parent == env.region,
            r.depth == match env.region {
                None => 0,
                Some(p) => p@.depth + 1,
            },
    {
        let tracked Env { region, .. } = env;
        let ghost depth: nat = if region is Some {
            region->0@.depth + 1
        } else {
            0
        };
        RegionPerms { blocks, headers, parent: region, depth }
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
    /// モードの前置きは生存の状態で、数えない印を持つ (docs/spec/object-model.md の「凍結した領域」)。`Foreign`
    /// の物体はない。塊はスレッドの間で共有するが、`Foreign` の値は `Send` しか求めないからである。
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
        &&& p_kind(p) != Kind::Foreign
        &&& p_extended(p) ==> cap >= 1 && p_init(p, 0)
        &&& p_first(p) + p_count(p) <= cap
        &&& p_kind(p).boxes_value() ==> cap >= 1 && p_init(p, 0)
        &&& p_kind(p).has_body() ==> body_wf(p, cap)
        &&& p.prefix matches Some(q) ==> q.first.is_init() && q.state.is_init()
        &&& p.prefix is Some ==> p_code(p) == 1 && p_owner(p) == UNCOUNTED
        &&& forall|i: int| #[trigger] p_scanned(p, i) ==> p_init(p, i) && self.word_ok(p_word(p, i))
    }

    /// `a` は、この塊か祖先の塊の権限である。
    pub(crate) open spec fn reaches(self, a: RegionPerms) -> bool
        decreases self.depth,
    {
        ||| self == a
        ||| self.parent matches Some(p) && p@.depth < self.depth && p@.reaches(a)
    }

    /// ページの頭 `a` は、この塊か祖先の塊のページの頭である。
    pub(crate) open spec fn has_page(self, a: usize) -> bool
        decreases self.depth,
    {
        ||| self.headers.contains_key(a)
        ||| self.parent matches Some(p) && p@.depth < self.depth && p@.has_page(a)
    }

    /// 鎖のブロックの集合。
    pub(crate) open spec fn block_set(self) -> Set<usize>
        decreases self.depth,
    {
        match self.parent {
            Some(p) if p@.depth < self.depth => self.blocks.dom().union(p@.block_set()),
            _ => self.blocks.dom(),
        }
    }

    /// 鎖のブロックの集合は、鎖のブロック (`has`) からちょうどなる。
    pub(crate) proof fn lemma_block_set(self)
        ensures
            forall|h: usize| #[trigger] self.block_set().contains(h) == self.has(h),
        decreases self.depth,
    {
        if let Some(p) = self.parent {
            if p@.depth < self.depth {
                p@.lemma_block_set();
                assert forall|h: usize| #[trigger] self.block_set().contains(h) == self.has(h) by {
                    assert(p@.block_set().contains(h) == p@.has(h));
                }
            }
        }
    }

    /// 祖先の塊のブロックとページは、この塊から見ても同じ塊のものである。鎖の塊のブロックは重ならないからである。
    pub(crate) proof fn lemma_reaches(self, a: RegionPerms, prefixed: bool)
        requires
            self.wf(prefixed),
            self.reaches(a),
        ensures
            a.wf(prefixed),
            forall|t: usize| #[trigger] a.has(t) ==> self.has(t) && self.owner(t) == a.owner(t),
            forall|x: usize| #[trigger] a.has_page(x) ==> self.has_page(x),
        decreases self.depth,
    {
        if self != a {
            let p = self.parent->0@;
            p.lemma_reaches(a, prefixed);
            assert forall|t: usize| #[trigger] a.has(t) implies self.has(t) && self.owner(t)
                == a.owner(t) by {
                assert(p.has(t));
                assert(!self.blocks.dom().contains(t));
            }
            assert forall|x: usize| #[trigger] a.has_page(x) implies self.has_page(x) by {
                assert(p.has_page(x));
            }
        }
    }

    /// この塊から祖先の塊 `mid` に届き、`mid` に親があれば、その親にも届く。
    pub(crate) proof fn lemma_reaches_parent(self, mid: RegionPerms, prefixed: bool)
        requires
            self.wf(prefixed),
            self.reaches(mid),
            mid.parent is Some,
        ensures
            self.reaches(mid.parent->0@),
            mid.parent->0@.depth < mid.depth,
        decreases self.depth,
    {
        self.lemma_reaches(mid, prefixed);
        if self == mid {
            assert(self.parent->0@.reaches(self.parent->0@));
        } else {
            let p = self.parent->0@;
            p.lemma_reaches_parent(mid, prefixed);
        }
    }

    /// ブロック `h` を持つ塊は、この塊か祖先の塊で、`h` をそのブロックに持つ。
    pub(crate) proof fn lemma_owner(self, h: usize, prefixed: bool)
        requires
            self.wf(prefixed),
            self.has(h),
        ensures
            self.reaches(self.owner(h)),
            self.owner(h).wf(prefixed),
            self.owner(h).blocks.dom().contains(h),
        decreases self.depth,
    {
        if !self.blocks.dom().contains(h) {
            self.parent->0@.lemma_owner(h, prefixed);
        }
    }

    /// 鎖のブロック `h` の形。ブロックの条件はそれを持つ塊から読み、走査する語は、この塊から見ても鎖のブロックを指す。
    pub(crate) proof fn lemma_block(self, h: usize, prefixed: bool)
        requires
            self.wf(prefixed),
            self.has(h),
        ensures
            self.owner(h).block_wf(h, prefixed),
            self.owner(h).blocks[h] == self.perm(h),
            region_ptr(self.owner(h).headers, h) == self.block(h),
            region_cap(self.owner(h).headers, h, prefixed) == self.cap(h, prefixed),
            self.has_page(page_base(h)),
            forall|i: int| #[trigger]
                p_scanned(self.perm(h), i) ==> p_init(self.perm(h), i) && self.word_ok(
                    p_word(self.perm(h), i),
                ),
    {
        self.lemma_owner(h, prefixed);
        let o = self.owner(h);
        assert(o.block_wf(h, prefixed));
        self.lemma_reaches(o, prefixed);
        assert(o.has_page(page_base(h)));
        assert forall|i: int| #[trigger] p_scanned(self.perm(h), i) implies p_init(self.perm(h), i)
            && self.word_ok(p_word(self.perm(h), i)) by {
            let w = p_word(o.blocks[h], i);
            assert(o.word_ok(w));
            if w.is_ptr_spec() {
                assert(o.has(w.bits_spec()));
            }
        }
    }

    /// ブロック `h` の権限を、それを持つ塊から借りる。
    pub(crate) proof fn tracked_perm(tracked &self, h: usize, prefixed: bool) -> (tracked p:
        &BlockPerm)
        requires
            self.wf(prefixed),
            self.has(h),
        ensures
            *p == self.perm(h),
        decreases self.depth,
    {
        if self.blocks.dom().contains(h) {
            self.blocks.tracked_borrow(h)
        } else {
            self.parent.tracked_borrow().borrow().tracked_perm(h, prefixed)
        }
    }

    /// ブロック `h` のページの頭の権限を、それを持つ塊から借りる。
    pub(crate) proof fn tracked_header(tracked &self, h: usize, prefixed: bool) -> (tracked r:
        &PointsTo<PageHeader>)
        requires
            self.wf(prefixed),
            self.has(h),
        ensures
            *r == self.owner(h).headers[page_base(h)],
        decreases self.depth,
    {
        if self.blocks.dom().contains(h) {
            assert(self.block_wf(h, prefixed));
            self.headers.tracked_borrow(page_base(h))
        } else {
            self.parent.tracked_borrow().borrow().tracked_header(h, prefixed)
        }
    }

    /// 持っているページの頭の権限 `hdr` のページは、鎖のページでない。
    pub(crate) proof fn lemma_apart(
        tracked &self,
        tracked hdr: &mut PointsTo<PageHeader>,
        prefixed: bool,
    )
        requires
            self.wf(prefixed),
        ensures
            *final(hdr) == *old(hdr),
            !self.has_page(old(hdr).ptr()@.addr),
        decreases self.depth,
    {
        let a = hdr.ptr()@.addr;
        lemma_header_size();
        if self.headers.contains_key(a) {
            hdr.is_disjoint(self.headers.tracked_borrow(a));
        }
        if self.parent is Some {
            let tracked p = self.parent.tracked_borrow().borrow();
            p.lemma_apart(hdr, prefixed);
        }
    }

    /// 塊とその祖先の不変条件。この塊のブロックは祖先のブロックと重ならない。
    pub(crate) open spec fn wf(self, prefixed: bool) -> bool
        decreases self.depth,
    {
        &&& forall|a: usize| #[trigger]
            self.headers.contains_key(a) ==> self.headers[a].ptr()@.addr == a
                && self.headers[a].is_init() && header_ok(self.headers[a].value(), prefixed)
                && header_sink(self.headers[a].value())@.addr == 0
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
/// 「凍結した領域」)。
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

    /// 塊とその祖先の権限。
    pub(crate) closed spec fn view_perms(&self) -> RegionPerms {
        self.perms@@
    }

    /// 作り手から移したページとブロックの権限で塊を作る。`parent` は、作り手が付けていた塊である。
    pub(crate) fn new(
        pages: Pages,
        parent: Option<Arc<Frozen<M>>>,
        Tracked(perms): Tracked<RegionPerms>,
    ) -> (r: Frozen<M>)
        requires
            perms.wf(M::CHECKED),
            match parent {
                None => perms.parent is None,
                Some(p) => perms.parent matches Some(s) && s@ == p.view_perms(),
            },
        ensures
            r.inv(),
            r.view_perms() == perms,
    {
        #[cfg(verus_keep_ghost)]
        let tracked shared = Shared::new(perms);
        Frozen {
            pages,
            parent,
            #[cfg(verus_keep_ghost)]
            perms: Tracked(shared),
            _mode: PhantomData,
        }
    }

    /// 塊の権限は、塊とその祖先の不変条件を満たす。
    pub(crate) proof fn lemma_wf(tracked &self)
        ensures
            self.view_perms().wf(M::CHECKED),
    {
        use_type_invariant(self);
    }

    /// 塊の権限を借りる。
    pub(crate) proof fn tracked_perms(tracked &self) -> (tracked r: &RegionPerms)
        ensures
            *r == self.view_perms(),
    {
        self.perms.borrow().borrow()
    }

    /// この塊を付けたヒープの `Env`。塊の権限の写しを持つ。写しは `Arc` が生かす塊のメモリを読むので、ヒープは写しと
    /// 一緒に `Arc` を持つ。
    pub(crate) proof fn env(tracked &self) -> (tracked e: Env)
        ensures
            e.checked == M::CHECKED,
            e.region matches Some(s) && s@ == self.view_perms(),
    {
        Env {
            checked: M::CHECKED,
            #[cfg(verus_keep_ghost)]
            region: Some(self.perms.borrow().clone()),
        }
    }

    /// `anc` は、この塊か祖先の塊である。親の `Arc` をたどり、塊の番地で比べる。
    pub(crate) fn descends_from(&self, anc: &Frozen<M>) -> (r: bool)
        ensures
            r ==> self.view_perms().reaches(anc.view_perms()),
    {
        let mut cur: &Frozen<M> = self;
        proof {
            use_type_invariant(self);
        }
        loop
            invariant
                self.view_perms().wf(M::CHECKED),
                self.view_perms().reaches(cur.view_perms()),
            decreases cur.view_perms().depth,
        {
            if same_chunk(cur, anc) {
                return true;
            }
            proof {
                use_type_invariant(cur);
            }
            match &cur.parent {
                None => return false,
                Some(p) => {
                    proof {
                        self.view_perms().lemma_reaches_parent(cur.view_perms(), M::CHECKED);
                    }
                    cur = &**p;
                },
            }
        }
    }
}

/// 2つの塊が同じ塊か。番地で比べる。vstd は `Arc` の番地の比較に約束を持たない (信頼の一覧の T5)。同じ番地の塊は
/// 同じ値である。
#[verifier::external_body]
fn same_chunk<M: Mode>(a: &Frozen<M>, b: &Frozen<M>) -> (r: bool)
    ensures
        r ==> *a == *b,
{
    std::ptr::eq(a, b)
}

/// 番地 `a` のブロックのページが、付けた塊 `region` かその祖先のページで、その塊を凍結した `Control` のページの表に
/// 登録してあるか (信頼の一覧の T15)。塊のページは、作り手のヒープが取ったときにその `Control` に登録し、塊を落とす
/// まで外さないので、塊とその祖先のブロックには真を返す。鎖を先頭からたどるので、付けたヒープの `Control` の表で
/// 見つからなかった語にだけ使う。
#[verifier::external_body]
pub(crate) fn region_known<M: Mode>(region: &Option<Arc<Frozen<M>>>, a: usize) -> (r: bool)
    ensures
        region matches Some(f) && (*f).view_perms().has(a) ==> r,
{
    let mut next = region.as_deref();
    while let Some(r) = next {
        if control_knows(r.pages.control(), a) {
            return true;
        }
        next = r.parent.as_deref();
    }
    false
}

/// 付けている塊の子孫でない塊を付けようとした。重ならないことを示せるのは鎖でつながる塊のブロックだけなので、
/// 誤った使い方として止める (信頼の一覧の T5)。状態を変える前に呼ぶ (T7)。
#[verifier::external_body]
#[cold]
pub(crate) fn not_a_descendant() -> ! {
    panic!("the attached region is not a descendant of the current one")
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
// 「`unsafe` の核」の T0)。ページの一覧の生のポインタは、最後の `Arc` を落とした `Drop` だけが使う
unsafe impl<M: Mode> Send for Frozen<M> {}
unsafe impl<M: Mode> Sync for Frozen<M> {}
