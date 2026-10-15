//! ヒープの不変条件と、API の前提の述語 (docs/spec/object-model.md の「`unsafe` の核」)。

use vstd::prelude::*;
#[cfg(verus_only)]
use vstd::raw_ptr::PointsTo;
#[cfg(verus_only)]
use vstd::utf8::valid_utf8;

use super::Core;
#[cfg(verus_only)]
use super::model::{BlockState, Pos, indeg};
#[cfg(verus_only)]
use crate::frozen::RegionPerms;
#[cfg(verus_only)]
use crate::meta::{BlockPerm, Header, Kind, bits_kind, bits_lin, bits_scan, bits_wf};
#[cfg(verus_only)]
use crate::rc::{magnitude, pinned};
#[cfg(verus_only)]
use crate::word::Word;

verus! {

// ブロックの権限の読み取り。ヒープの述語は、どれもこの言葉で書く
pub(crate) open spec fn p_meta(p: BlockPerm) -> u32 {
    p.meta.value()
}

pub(crate) open spec fn p_kind(p: BlockPerm) -> Kind {
    bits_kind(p.meta.value())
}

pub(crate) open spec fn p_extended(p: BlockPerm) -> bool {
    bits_scan(p.meta.value()) == 255
}

/// 検査モードの前置きの状態の欄の、状態の2ビット。
pub(crate) open spec fn p_code(p: BlockPerm) -> u32 {
    p.prefix->0.state.value() & 3
}

/// 検査モードの前置きの状態の欄の、世代の番号。
pub(crate) open spec fn p_gen(p: BlockPerm) -> u32 {
    p.prefix->0.state.value() >> 2
}

/// 検査モードの前置きの `first` の欄 (所有者 ID か不死の物体の数)。
pub(crate) open spec fn p_owner(p: BlockPerm) -> u32 {
    p.prefix->0.first.value()
}

/// 前置きは、不死の物体 (`immortal`) の `first` の欄 (外に出ている参照の数) のほかは変わらない。
pub(crate) open spec fn prefix_recounted(x: BlockPerm, y: BlockPerm, immortal: bool) -> bool {
    if immortal {
        &&& (x.prefix is Some) == (y.prefix is Some)
        &&& x.prefix matches Some(q) ==> {
            &&& q.state == y.prefix->0.state
            &&& q.first.ptr() == y.prefix->0.first.ptr()
            &&& q.first.is_init()
        }
    } else {
        x.prefix == y.prefix
    }
}

/// 前置きの欄の位置と所有者の値は同じで、状態の欄は書いてある。状態の欄の値は変わってよい。
pub(crate) open spec fn prefix_kept(x: BlockPerm, y: BlockPerm) -> bool {
    &&& (x.prefix is Some) == (y.prefix is Some)
    &&& x.prefix matches Some(q) ==> {
        &&& q.first == y.prefix->0.first
        &&& q.state.ptr() == y.prefix->0.state.ptr()
        &&& q.state.is_init()
    }
}

/// ブロックの状態を表す前置きの状態の2ビット (docs/spec/object-model.md の「前置き」)。
pub(crate) open spec fn state_code(st: BlockState) -> u32 {
    match st {
        BlockState::Building | BlockState::Live => 1,
        BlockState::Token => 2,
        BlockState::Dying => 0,
        BlockState::Freed => 3,
    }
}

/// i 語目は書いてある。
pub(crate) open spec fn p_init(p: BlockPerm, i: int) -> bool {
    p.slots.contains_key(i) && p.slots[i].is_init()
}

pub(crate) open spec fn p_word(p: BlockPerm, i: int) -> Word {
    p.slots[i].value()
}

/// 走査する語の始まり (`scan` が 255 なら、数の語の次)。
pub(crate) open spec fn p_first(p: BlockPerm) -> int {
    if p_extended(p) {
        1
    } else {
        0
    }
}

/// 走査する語の数。`scan` が 255 なら、ペイロードの先頭の数の語から読む。
pub(crate) open spec fn p_count(p: BlockPerm) -> int {
    if p_extended(p) {
        p_word(p, 0).bits_spec() as int
    } else {
        bits_scan(p.meta.value()) as int
    }
}

/// `String` の物体のバイト列。
pub(crate) open spec fn p_text(p: BlockPerm) -> Seq<u8> {
    p.text()
}

/// `String` の物体の中身 (docs/spec/object-model.md の「文字列」)。走査する語はなく、生の 0 語目にバイト長を持ち、
/// バイト列は長さの語の後ろの容量に収まる妥当な UTF-8 である。
pub(crate) open spec fn string_wf(p: BlockPerm, cap: nat) -> bool {
    &&& p_count(p) == 0
    &&& !p_extended(p)
    &&& cap >= 1
    &&& p_init(p, 0)
    &&& p_word(p, 0).bits_spec() == p.text().len()
    &&& p.text().len() <= 8 * (cap - 1)
    &&& valid_utf8(p.text())
}

/// ペイロードの語は、容量の終わりまですべて書いてある。`Data` の物体は生の語の数を持たないので、回復が生の語を容量の
/// 終わりまで写せるように、`Data` のブロックはいつもこの形にする
/// (docs/spec/object-model.md の「確保と解放」)。
pub(crate) open spec fn p_filled(p: BlockPerm, cap: nat) -> bool {
    slots_filled(p.slots, cap)
}

/// 語の権限の表を引数に取るのは、表が同じなら `p_filled` も同じだと量化子を開かずに分かるようにするためである。
pub(crate) open spec fn slots_filled(slots: Map<int, PointsTo<Word>>, cap: nat) -> bool {
    forall|i: int| 0 <= i < cap ==> slots.contains_key(i) && (#[trigger] slots[i]).is_init()
}

/// どの語も書いてあれば、`p_filled` である。
pub(crate) proof fn lemma_filled(p: BlockPerm, cap: nat)
    requires
        forall|i: int| 0 <= i < cap ==> #[trigger] p_init(p, i),
    ensures
        p_filled(p, cap),
{
    assert forall|i: int| 0 <= i < cap implies p.slots.contains_key(i) && (
    #[trigger] p.slots[i]).is_init() by {
        assert(p_init(p, i));
    }
}

/// `p_filled` なら、容量の中のどの語も書いてある。
pub(crate) proof fn lemma_filled_init(p: BlockPerm, cap: nat, i: int)
    requires
        p_filled(p, cap),
        0 <= i < cap,
    ensures
        p_init(p, i),
{
    assert(p.slots.contains_key(i) && p.slots[i].is_init());
}

/// i 語目は走査する語である。
pub(crate) open spec fn p_scanned(p: BlockPerm, i: int) -> bool {
    p_first(p) <= i < p_first(p) + p_count(p)
}

impl Core {
    /// 検査モードか。
    pub(crate) open spec fn checked(&self) -> bool {
        self.env@.checked
    }

    // ブロックの読み取り
    pub(crate) open spec fn dom(&self) -> Set<usize> {
        self.perms@.dom()
    }

    /// 付けた塊の権限。塊を付けていなければ意味を持たない。
    pub(crate) open spec fn region(&self) -> RegionPerms {
        self.env@.region->0@
    }

    /// `h` は、付けた塊か祖先の塊のブロックである。
    pub(crate) open spec fn region_has(&self, h: usize) -> bool {
        self.env@.region matches Some(r) && r@.has(h)
    }

    /// 付けた塊とその祖先のブロックの集合。
    pub(crate) open spec fn region_blocks(&self) -> Set<usize> {
        match self.env@.region {
            Some(r) => r@.block_set(),
            None => Set::empty(),
        }
    }

    /// 付けた塊のブロックの集合は、付けた塊のブロック (`region_has`) からちょうどなる。
    pub(crate) proof fn lemma_region_blocks(&self)
        ensures
            forall|h: usize| #[trigger] self.region_blocks().contains(h) == self.region_has(h),
    {
        if let Some(r) = self.env@.region {
            r@.lemma_block_set();
        }
    }

    /// `h` は塊のブロックで、このヒープのブロックでない。このときブロックの読み取りは、塊の権限を読む。
    pub(crate) open spec fn in_region(&self, h: usize) -> bool {
        !self.dom().contains(h) && self.region_has(h)
    }

    /// `h` は、このヒープのブロックか、付けた塊とその祖先のブロックである。
    pub(crate) open spec fn has(&self, h: usize) -> bool {
        self.dom().contains(h) || self.region_has(h)
    }

    pub(crate) open spec fn perm(&self, h: usize) -> BlockPerm {
        if self.in_region(h) {
            self.region().perm(h)
        } else {
            self.perms@[h]
        }
    }

    /// 塊のブロックは、いつも生きている物体である。
    pub(crate) open spec fn state(&self, h: usize) -> BlockState {
        if self.in_region(h) {
            BlockState::Live
        } else {
            self.model@.state[h]
        }
    }

    pub(crate) open spec fn block(&self, h: usize) -> *mut Header {
        if self.in_region(h) {
            self.region().block(h)
        } else {
            self.pages.block_ptr(h)
        }
    }

    pub(crate) open spec fn cap(&self, h: usize) -> nat {
        if self.in_region(h) {
            self.region().cap(h, self.checked())
        } else {
            self.pages.cap(h)
        }
    }

    pub(crate) open spec fn rc_of(&self, h: usize) -> i32 {
        self.perm(h).rc.value()
    }

    pub(crate) open spec fn meta_bits(&self, h: usize) -> u32 {
        p_meta(self.perm(h))
    }

    pub(crate) open spec fn kind(&self, h: usize) -> Kind {
        p_kind(self.perm(h))
    }

    pub(crate) open spec fn lin(&self, h: usize) -> bool {
        bits_lin(self.meta_bits(h))
    }

    pub(crate) open spec fn count(&self, h: usize) -> int {
        p_count(self.perm(h))
    }

    pub(crate) open spec fn scanned(&self, h: usize, i: int) -> bool {
        p_scanned(self.perm(h), i)
    }

    /// 語 `w` は即値か、このヒープか付けた塊のブロックを、そのブロックの provenance で指す。
    pub(crate) open spec fn word_ok(&self, w: Word) -> bool {
        w.is_ptr_spec() ==> {
            &&& self.has(w.bits_spec())
            &&& w.ptr_spec()@.provenance == self.block(w.bits_spec())@.provenance
        }
    }

    /// 物体を指す辺の数。
    pub(crate) open spec fn indeg(&self, h: usize) -> nat {
        indeg(self.model@.edges, h)
    }

    /// 外の呼び出し側が持つ、数えた参照の数。
    pub(crate) open spec fn ext(&self, h: usize) -> int {
        self.model@.refs[h] - self.indeg(h)
    }

    // API の前提
    /// `w` は即値か、生きている物体 (書き終えた構築中の物体を含む) を指す。借りた参照も含む。
    pub(crate) open spec fn readable(&self, w: Word) -> bool {
        &&& self.word_ok(w)
        &&& w.is_ptr_spec() ==> self.settled_state(w.bits_spec()) == BlockState::Live
    }

    /// `w` は即値か、生きている物体を指し、呼ぶ側が数えた参照を1つ持つ (不死の物体では数えない)。
    pub(crate) open spec fn holds(&self, w: Word) -> bool {
        &&& self.readable(w)
        &&& w.is_ptr_spec() && self.rc_of(w.bits_spec()) != 0 ==> self.ext(w.bits_spec()) >= 1
    }

    /// 構築中の物体 `b` の「残した」語のうち、物体を指すもの。構築中の物体を生きている物体にするときに辺になる。
    pub(crate) open spec fn kept_edges(&self, b: usize) -> Map<Pos, usize> {
        let p = self.perm(b);
        Map::new(
            self.model@.kept.filter(|e: Pos| e.0 == b && p_word(p, e.1).is_ptr_spec()),
            |e: Pos| p_word(p, e.1).bits_spec(),
        )
    }

    /// 構築中の物体 `b` の「残した」語 `w` は、即値か、生きている物体か `b` 自身を指す。
    pub(crate) open spec fn kept_ok(&self, b: usize, w: Word) -> bool {
        &&& self.word_ok(w)
        &&& w.is_ptr_spec() ==> self.state(w.bits_spec()) == BlockState::Live || w.bits_spec() == b
    }

    /// 構築中の物体はないか、書き終えている。書き終えたとは、走査する語がどれも「書いた」か「残した」で、`BigInt`
    /// なら値の語が書いてあることである。「残した」語の参照は呼ぶ側が持つ。
    pub(crate) open spec fn settleable(&self) -> bool {
        match self.model@.building {
            None => true,
            Some(b) => {
                &&& forall|i: int| #[trigger]
                    self.scanned(b, i) ==> self.model@.written[b].contains(i)
                        || self.model@.kept.contains((b, i))
                &&& self.kind(b) == Kind::BigInt ==> p_init(self.perm(b), 0)
                &&& forall|i: int| #[trigger]
                    self.model@.kept.contains((b, i)) ==> self.kept_ok(b, p_word(self.perm(b), i))
                &&& forall|t: usize|
                    #![trigger indeg(self.kept_edges(b), t)]
                    indeg(self.kept_edges(b), t) > 0 && self.rc_of(t) != 0 ==> self.model@.refs[t]
                        >= self.indeg(t) + indeg(self.kept_edges(b), t)
            },
        }
    }

    /// `old` から、ブロック `h` の検査モードの前置きの状態の欄の値だけが変わった。
    pub(crate) open spec fn prefix_moved(&self, old: Core, h: usize) -> bool {
        &&& self.perms@.dom() == old.perms@.dom()
        &&& forall|b: usize| b != h ==> #[trigger] self.perms@[b] == old.perms@[b]
        &&& self.perm(h).rc == old.perm(h).rc
        &&& self.perm(h).meta == old.perm(h).meta
        &&& self.perm(h).slots == old.perm(h).slots
        &&& self.perm(h).text == old.perm(h).text
        &&& prefix_kept(self.perm(h), old.perm(h))
    }

    // 不変条件
    /// モデルとの対応を含む不変条件。速いモードでは安全の条件でもある。
    pub(crate) open spec fn full(&self) -> bool {
        &&& self.dom() == self.pages.in_use()
        &&& self.rest_wf()
    }

    /// 確保器から取ったブロック `h` を、まだヒープに入れていない。
    pub(crate) open spec fn pending(&self, h: usize) -> bool {
        &&& self.pages.in_use() == self.dom().insert(h)
        &&& !self.dom().contains(h)
        &&& self.rest_wf()
    }

    pub(crate) open spec fn rest_wf(&self) -> bool {
        &&& self.pages.wf()
        &&& self.pages.prefixed() == self.checked()
        &&& self.env_wf()
        &&& self.blocks_wf()
        &&& self.edges_wf()
        &&& self.refs_wf()
        &&& self.building_wf()
    }

    /// 付けた塊は、塊とその祖先の不変条件を満たす。
    pub(crate) open spec fn region_wf(&self) -> bool {
        self.env@.region matches Some(r) ==> r@.wf(self.checked())
    }

    /// 付けた塊は不変条件を満たし、そのブロックは確保器が渡したブロックと重ならない。
    pub(crate) open spec fn env_wf(&self) -> bool {
        &&& self.region_wf()
        &&& forall|h: usize|
            #![trigger self.region_has(h)]
            #![trigger self.pages.in_use().contains(h)]
            self.region_has(h) ==> !self.pages.in_use().contains(h)
    }

    #[verifier::opaque]
    pub(crate) open spec fn blocks_wf(&self) -> bool {
        &&& self.model@.state.dom() == self.dom()
        &&& self.model@.refs.dom() == self.dom()
        &&& self.model@.stuck.dom() == self.dom()
        &&& self.model@.owner.dom() == self.dom()
        &&& self.model@.written.dom() == self.dom()
        &&& forall|h: usize| #[trigger] self.dom().contains(h) ==> self.block_wf(h)
        &&& forall|e: Pos| #[trigger] self.model@.kept.contains(e) ==> self.kept_wf(e)
    }

    pub(crate) open spec fn block_wf(&self, h: usize) -> bool {
        &&& self.pages.carved_block(h)
        &&& self.perm_wf(
            self.perm(h),
            h,
            self.state(h),
            self.model@.written[h],
            self.model@.owner[h],
        )
    }

    /// 「残した」語の位置は、token の箱か構築中の物体の、書いてある走査する語である。
    pub(crate) open spec fn kept_wf(&self, e: Pos) -> bool {
        &&& self.dom().contains(e.0)
        &&& self.state(e.0) == BlockState::Building || self.state(e.0) == BlockState::Token
        &&& self.scanned(e.0, e.1)
        &&& p_init(self.perm(e.0), e.1)
    }

    /// ブロック `h` の権限 `p` が、状態 `st` の物体として正しい。`written` は構築中の物体の書いた語で、`owner` は持ち主
    /// である。検査モードの前置きの状態は物体の状態を表し、rc が正の物体の前置きは持ち主を持つ。
    pub(crate) open spec fn perm_wf(
        &self,
        p: BlockPerm,
        h: usize,
        st: BlockState,
        written: Set<int>,
        owner: u32,
    ) -> bool {
        &&& p.wf(self.block(h), self.cap(h), self.checked())
        &&& p.prefix matches Some(q) ==> q.first.is_init() && q.state.is_init()
        &&& p.prefix is Some ==> p_code(p) == state_code(st)
        &&& p.prefix is Some && (st == BlockState::Live || st == BlockState::Building)
            && p.rc.value() > 0 ==> p_owner(p) == owner
        &&& p.rc.is_init()
        &&& p.meta.is_init()
        &&& bits_wf(p_meta(p))
        &&& p_kind(p) != Kind::Free
        &&& st != BlockState::Freed ==> {
            &&& p_extended(p) ==> self.cap(h) >= 1 && p_init(p, 0)
            &&& p_first(p) + p_count(p) <= self.cap(h)
        }
        &&& st == BlockState::Live || st == BlockState::Dying ==> (p_kind(p) == Kind::BigInt
            ==> self.cap(h) >= 1 && p_init(p, 0))
        &&& st == BlockState::Live || st == BlockState::Dying ==> (p_kind(p) == Kind::String
            ==> string_wf(p, self.cap(h)))
        &&& st != BlockState::Freed && p_kind(p) == Kind::Data ==> p_filled(p, self.cap(h))
        &&& st == BlockState::Building ==> p_kind(p) != Kind::String
        &&& st == BlockState::Live ==> forall|i: int| #[trigger]
            p_scanned(p, i) ==> self.slot_word(p, h, i)
        &&& st == BlockState::Dying ==> forall|i: int| #[trigger] p_scanned(p, i) ==> p_init(p, i)
        &&& st == BlockState::Building ==> forall|i: int| #[trigger]
            written.contains(i) ==> p_scanned(p, i) && self.slot_word(p, h, i)
                && !self.model@.kept.contains((h, i))
        &&& st == BlockState::Token ==> p_kind(p) == Kind::Data && p.rc.value() == 1
        &&& st == BlockState::Token ==> forall|i: int| #[trigger] p_scanned(p, i) ==> p_init(p, i)
    }

    /// i 語目は書いてあり、値は即値か、このヒープか付けた塊のブロックを指す。ポインタなら辺である。
    pub(crate) open spec fn slot_word(&self, p: BlockPerm, h: usize, i: int) -> bool {
        &&& p_init(p, i)
        &&& self.word_ok(p_word(p, i))
        &&& p_word(p, i).is_ptr_spec() ==> self.model@.edges.dom().contains((h, i))
    }

    #[verifier::opaque]
    pub(crate) open spec fn edges_wf(&self) -> bool {
        forall|e: Pos| #[trigger] self.model@.edges.dom().contains(e) ==> self.edge_wf(e)
    }

    /// 辺は、生きている物体、構築中の物体の書いた語、空ける途中の物体のまだ手放していない語から出る。行き先は、このヒープか
    /// 付けた塊の、生きている物体か構築中の物体である。
    pub(crate) open spec fn edge_wf(&self, e: Pos) -> bool {
        let (h, i) = e;
        let t = self.model@.edges[e];
        &&& self.dom().contains(h)
        &&& self.state(h) == BlockState::Live || self.state(h) == BlockState::Building
            || self.state(h) == BlockState::Dying
        &&& self.state(h) == BlockState::Building ==> self.model@.written[h].contains(i)
        &&& self.scanned(h, i)
        &&& p_init(self.perm(h), i)
        &&& p_word(self.perm(h), i).is_ptr_spec()
        &&& p_word(self.perm(h), i).bits_spec() == t
        &&& p_word(self.perm(h), i).ptr_spec()@.provenance == self.block(t)@.provenance
        &&& self.has(t)
        &&& (self.state(t) == BlockState::Live || self.state(t) == BlockState::Building)
    }

    #[verifier::opaque]
    pub(crate) open spec fn refs_wf(&self) -> bool {
        forall|h: usize| #[trigger] self.dom().contains(h) ==> self.counted_wf(h)
    }

    /// rc とモデルの参照の数が対応する (`rc.rs` の理想のモデルとの対応と同じ形)。
    pub(crate) open spec fn counted_wf(&self, h: usize) -> bool {
        let rc = self.rc_of(h);
        &&& self.state(h) == BlockState::Dying || self.state(h) == BlockState::Token ==> self.indeg(
            h,
        ) == 0
        &&& (self.state(h) == BlockState::Live || self.state(h) == BlockState::Building) && rc != 0
            ==> {
            &&& self.model@.refs[h] >= self.indeg(h)
            &&& self.model@.stuck[h] ==> pinned(rc)
            &&& !self.model@.stuck[h] ==> !pinned(rc) && magnitude(rc) == self.model@.refs[h]
                && self.model@.refs[h] > 0
        }
    }

    #[verifier::opaque]
    pub(crate) open spec fn building_wf(&self) -> bool {
        &&& self.model@.building matches Some(b) ==> self.dom().contains(b) && self.state(b)
            == BlockState::Building
        &&& forall|h: usize| #[trigger]
            self.dom().contains(h) && self.state(h) == BlockState::Building ==> self.model@.building
                == Some(h)
    }
}

} // verus!
