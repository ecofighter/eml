//! ヒープの不変条件と、API の前提の述語 (docs/spec/object-model.md の「`unsafe` の核」)。

use vstd::prelude::*;

use super::Core;
#[cfg(verus_only)]
use super::model::{BlockState, Pos, indeg};
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

/// i 語目は走査する語である。
pub(crate) open spec fn p_scanned(p: BlockPerm, i: int) -> bool {
    p_first(p) <= i < p_first(p) + p_count(p)
}

impl Core {
    // ブロックの読み取り
    pub(crate) open spec fn dom(&self) -> Set<usize> {
        self.perms@.dom()
    }

    pub(crate) open spec fn perm(&self, h: usize) -> BlockPerm {
        self.perms@[h]
    }

    pub(crate) open spec fn state(&self, h: usize) -> BlockState {
        self.model@.state[h]
    }

    pub(crate) open spec fn block(&self, h: usize) -> *mut Header {
        self.pages.block_ptr(h)
    }

    pub(crate) open spec fn cap(&self, h: usize) -> nat {
        self.pages.cap(h)
    }

    pub(crate) open spec fn rc_of(&self, h: usize) -> i32 {
        self.perms@[h].rc.value()
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

    /// 語 `w` は即値か、このヒープのブロックを、そのブロックの provenance で指す。
    pub(crate) open spec fn word_ok(&self, w: Word) -> bool {
        w.is_ptr_spec() ==> {
            &&& self.dom().contains(w.bits_spec())
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

    /// 構築中の物体はないか、書き終えている。
    pub(crate) open spec fn settleable(&self) -> bool {
        match self.model@.building {
            None => true,
            Some(b) => {
                &&& forall|i: int| #[trigger]
                    self.scanned(b, i) ==> self.model@.written[b].contains(i)
                &&& self.kind(b) == Kind::BigInt ==> p_init(self.perm(b), 0)
            },
        }
    }

    // 不変条件
    /// 不変条件。
    pub(crate) open spec fn wf(&self) -> bool {
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
        &&& self.pages.prefixed() == self.checked@
        &&& self.blocks_wf()
        &&& self.edges_wf()
        &&& self.refs_wf()
        &&& self.building_wf()
    }

    #[verifier::opaque]
    pub(crate) open spec fn blocks_wf(&self) -> bool {
        &&& self.model@.state.dom() == self.dom()
        &&& self.model@.refs.dom() == self.dom()
        &&& self.model@.stuck.dom() == self.dom()
        &&& self.model@.owner.dom() == self.dom()
        &&& self.model@.written.dom() == self.dom()
        &&& forall|h: usize| #[trigger] self.dom().contains(h) ==> self.block_wf(h)
    }

    pub(crate) open spec fn block_wf(&self, h: usize) -> bool {
        &&& self.pages.carved_block(h)
        &&& self.perm_wf(self.perm(h), h, self.state(h), self.model@.written[h])
    }

    /// ブロック `h` の権限 `p` が、状態 `st` の物体として正しい。`written` は構築中の物体の書いた語である。
    pub(crate) open spec fn perm_wf(
        &self,
        p: BlockPerm,
        h: usize,
        st: BlockState,
        written: Set<int>,
    ) -> bool {
        &&& p.wf(self.block(h), self.cap(h), self.checked@)
        &&& p.prefix matches Some(q) ==> q.first.is_init() && q.state.is_init()
        &&& p.rc.is_init()
        &&& p.meta.is_init()
        &&& bits_wf(p_meta(p))
        &&& p_kind(p) != Kind::Free
        &&& st == BlockState::Building || st == BlockState::Live || st == BlockState::Dying || st
            == BlockState::Freed
        &&& st == BlockState::Live || st == BlockState::Dying ==> {
            &&& p_extended(p) ==> self.cap(h) >= 1 && p_init(p, 0)
            &&& p_kind(p) == Kind::BigInt ==> self.cap(h) >= 1 && p_init(p, 0)
        }
        &&& st == BlockState::Building ==> !p_extended(p)
        &&& st != BlockState::Freed ==> p_first(p) + p_count(p) <= self.cap(h)
        &&& st == BlockState::Live ==> forall|i: int| #[trigger]
            p_scanned(p, i) ==> self.slot_word(p, h, i)
        &&& st == BlockState::Dying ==> forall|i: int| #[trigger] p_scanned(p, i) ==> p_init(p, i)
        &&& st == BlockState::Building ==> forall|i: int| #[trigger]
            written.contains(i) ==> p_scanned(p, i) && self.slot_word(p, h, i)
    }

    /// i 語目は書いてあり、値は即値か、このヒープのブロックを指す。ポインタなら辺である。
    pub(crate) open spec fn slot_word(&self, p: BlockPerm, h: usize, i: int) -> bool {
        &&& p_init(p, i)
        &&& self.word_ok(p_word(p, i))
        &&& p_word(p, i).is_ptr_spec() ==> self.model@.edges.dom().contains((h, i))
    }

    #[verifier::opaque]
    pub(crate) open spec fn edges_wf(&self) -> bool {
        forall|e: Pos| #[trigger] self.model@.edges.dom().contains(e) ==> self.edge_wf(e)
    }

    pub(crate) open spec fn edge_wf(&self, e: Pos) -> bool {
        let (h, i) = e;
        let t = self.model@.edges[e];
        &&& self.dom().contains(h)
        &&& self.state(h) != BlockState::Freed
        &&& self.scanned(h, i)
        &&& p_init(self.perm(h), i)
        &&& p_word(self.perm(h), i).is_ptr_spec()
        &&& p_word(self.perm(h), i).bits_spec() == t
        &&& p_word(self.perm(h), i).ptr_spec()@.provenance == self.block(t)@.provenance
        &&& self.dom().contains(t)
        &&& (self.state(t) == BlockState::Live || self.state(t) == BlockState::Building)
    }

    #[verifier::opaque]
    pub(crate) open spec fn refs_wf(&self) -> bool {
        forall|h: usize| #[trigger] self.dom().contains(h) ==> self.counted_wf(h)
    }

    /// rc とモデルの参照の数が対応する (`rc.rs` の理想のモデルとの対応と同じ形)。
    pub(crate) open spec fn counted_wf(&self, h: usize) -> bool {
        let rc = self.rc_of(h);
        &&& self.state(h) == BlockState::Dying ==> self.indeg(h) == 0
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
