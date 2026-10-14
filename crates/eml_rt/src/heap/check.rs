//! 検査モードの前置きと検査 (docs/spec/object-model.md の「前置き」と「報告する
//! 誤り」)。前置きは検査モードのブロックにだけあるので、前置きを読む関数は `M::CHECKED` のときだけ働く。

#[cfg(test)]
use std::ptr::NonNull;

use super::Heap;
#[cfg(verus_only)]
use super::chain_err;
#[cfg(verus_only)]
use super::inv::{p_owner, prefix_recounted};
#[cfg(verus_only)]
use super::model::BlockState;
use crate::cell::{u32_load, u32_store};
use crate::error::HeapError;
#[cfg(verus_only)]
use crate::meta::field_ptr;
#[cfg(verus_only)]
use crate::meta::same_fields;
use crate::meta::{Header, Kind, Meta, PrefixPerm, prefix_first_ptr, prefix_state_ptr};
use crate::mode::Mode;
use std::sync::atomic::AtomicU32;
use vstd::prelude::*;
use vstd::raw_ptr::ptr_mut_write;

#[repr(C)]
pub(crate) struct Prefix {
    /// 普通の物体では所有者 ID、`alloc_immortal` の物体では外に出ている参照の数 (docs/spec/object-model.md の「不死の物体の数え方」)。
    pub(crate) first: AtomicU32,
    /// 下位2ビットが状態、残りが世代の番号。
    state: AtomicU32,
}

verus! {

/// 前置きの語の数。
pub(crate) const PREFIX_WORDS: usize = 1;

/// `first` のこの値の不死の物体 (ダミー) は、数を変えも確かめもしない。
pub(crate) const UNCOUNTED: u32 = 0xFFFF_FFFF;

/// 前置きの状態の欄の値。下位2ビットが状態、残りが世代の番号である。今確保したブロックは、世代 0 の生存にする。
pub(crate) const ALIVE_STATE: u32 = 1;

/// 今確保したブロックの前置きを書く。
#[inline]
pub(crate) fn init_prefix(h: *mut Header, first: u32, Tracked(perm): Tracked<&mut PrefixPerm>)
    requires
        h@.addr >= 8,
        old(perm).first.ptr() == field_ptr::<u32>(h, -8),
        old(perm).state.ptr() == field_ptr::<u32>(h, -4),
    ensures
        final(perm).first.ptr() == old(perm).first.ptr(),
        final(perm).state.ptr() == old(perm).state.ptr(),
        final(perm).first.is_init() && final(perm).first.value() == first,
        final(perm).state.is_init() && final(perm).state.value() == ALIVE_STATE,
{
    ptr_mut_write(prefix_first_ptr(h), Tracked(&mut perm.first), first);
    ptr_mut_write(prefix_state_ptr(h), Tracked(&mut perm.state), ALIVE_STATE);
}

/// 前置きの状態の「解放」の値。
pub(crate) const FREED_STATE: u32 = 3;

/// 前置きの状態を「解放」にし、世代の番号を1つ進める。
#[inline]
pub(crate) fn mark_freed(h: *mut Header, Tracked(perm): Tracked<&mut PrefixPerm>)
    requires
        h@.addr >= 8,
        old(perm).state.ptr() == field_ptr::<u32>(h, -4),
        old(perm).state.is_init(),
    ensures
        final(perm).first == old(perm).first,
        final(perm).state.ptr() == old(perm).state.ptr(),
        final(perm).state.is_init(),
        final(perm).state.value() & 3 == FREED_STATE,
{
    let p = prefix_state_ptr(h);
    let bits = u32_load(p, Tracked(&perm.state));
    let generation = (bits >> 2u32).wrapping_add(1);
    let v = (generation << 2u32) | FREED_STATE;
    proof {
        lemma_code(generation, FREED_STATE);
    }
    u32_store(p, Tracked(&mut perm.state), v);
}

/// 今確保したブロックの前置きの状態は、生存である。
pub(crate) proof fn lemma_alive_code()
    ensures
        ALIVE_STATE & 3 == 1,
{
    assert(1u32 & 3u32 == 1u32) by (bit_vector);
}

/// 状態の欄の世代の番号を保ったまま、状態の2ビットを `code` に替える。
pub(crate) proof fn lemma_set_code(v: u32, code: u32)
    requires
        code < 4,
    ensures
        ((v & !3u32) | code) & 3 == code,
        ((v & !3u32) | code) >> 2u32 == v >> 2u32,
        (v & !3u32) & 3 == 0,
{
    assert((v & !3u32) & 3 == 0) by (bit_vector);
    assert(((v & !3u32) | code) & 3 == code) by (bit_vector)
        requires
            code < 4,
    ;
    assert(((v & !3u32) | code) >> 2u32 == v >> 2u32) by (bit_vector)
        requires
            code < 4,
    ;
}

/// 世代の番号と状態の2ビットを並べた値の、下位2ビットは状態である。
pub(crate) proof fn lemma_code(generation: u32, code: u32)
    requires
        code < 4,
    ensures
        ((generation << 2u32) | code) & 3 == code,
        ((generation << 2u32) | code) >> 2u32 == generation % 0x4000_0000,
{
    assert(((generation << 2u32) | code) & 3 == code) by (bit_vector)
        requires
            code < 4,
    ;
    assert(((generation << 2u32) | code) >> 2u32 == generation % 0x4000_0000) by (bit_vector)
        requires
            code < 4,
    ;
}

// 検査モードの検査。速いモードでは何も確かめない
impl<M: Mode> Heap<M> {
    /// 前置きの状態が生存かを確かめる。解放した共有の物体は rc が 0 になっていて不死の物体と区別できないので、rc を
    /// 見る前に呼ぶ。
    #[inline]
    pub(super) fn checked_live(&self, h: *mut Header) -> (r: Result<(), HeapError>)
        requires
            self.core.sound(),
            self.core.checked@ == M::CHECKED,
            self.core.dom().contains(h@.addr),
            h == self.core.block(h@.addr),
        ensures
            !M::CHECKED ==> r is Ok,
            M::CHECKED ==> (r is Ok <==> (self.core.state(h@.addr) == BlockState::Live
                || self.core.state(h@.addr) == BlockState::Building)),
            r is Err ==> r == Err::<(), HeapError>(HeapError::UseAfterFree),
    {
        if !M::CHECKED {
            return Ok(());
        }
        let bits = self.core.prefix_state_get(h);
        proof {
            self.core.lemma_code_state(h@.addr);
        }
        if bits & 3 == ALIVE_STATE {
            Ok(())
        } else {
            Err(HeapError::UseAfterFree)
        }
    }

    /// 正の rc の物体は、今の worker が持ち主でなければならない (docs/spec/object-model.md の「論理的な worker ID」)。
    #[inline]
    pub(super) fn checked_owner(&self, h: *mut Header, rc: i32) -> (r: Result<(), HeapError>)
        requires
            self.core.sound(),
            self.core.checked@ == M::CHECKED,
            self.core.dom().contains(h@.addr),
            h == self.core.block(h@.addr),
        ensures
            r is Err ==> r matches Err(HeapError::WrongOwner { .. }),
            !M::CHECKED ==> r is Ok,
            M::CHECKED ==> (r is Ok <==> (rc <= 0 || p_owner(self.core.perm(h@.addr))
                == self.worker)),
    {
        if !M::CHECKED || rc <= 0 {
            return Ok(());
        }
        let owner = self.core.prefix_first_get(h);
        if owner == self.worker {
            Ok(())
        } else {
            Err(HeapError::WrongOwner { owner, worker: self.worker })
        }
    }

    /// 生きている不死の物体の参照を数えるか。数えるなら、あと1つ数えられることも確かめる。
    #[inline]
    pub(super) fn immortal_counted(&self, h: *mut Header) -> (r: bool)
        requires
            self.core.sound(),
            self.core.checked@,
            self.core.dom().contains(h@.addr),
            h == self.core.block(h@.addr),
        ensures
            self.core.perm(h@.addr).prefix is Some,
            r == (p_owner(self.core.perm(h@.addr)) != UNCOUNTED),
            r ==> p_owner(self.core.perm(h@.addr)) < UNCOUNTED - 1,
    {
        let n = self.core.prefix_first_get(h);
        if n == UNCOUNTED {
            return false;
        }
        if n == UNCOUNTED - 1 {
            too_many_acquires();
        }
        true
    }

    /// 不死の物体の外に出ている参照の数を1つ増やすか減らす。どちらの向きも、確かめるのは先に済ませてある。
    #[inline]
    pub(super) fn count_immortal(&mut self, h: *mut Header, up: bool)
        requires
            old(self).core.safe_wf(),
            old(self).core.checked@,
            old(self).core.dom().contains(h@.addr),
            h == old(self).core.block(h@.addr),
            old(self).core.rc_of(h@.addr) == 0,
            old(self).core.perm(h@.addr).prefix is Some,
            p_owner(old(self).core.perm(h@.addr)) != UNCOUNTED,
            up ==> p_owner(old(self).core.perm(h@.addr)) < UNCOUNTED - 1,
            !up ==> p_owner(old(self).core.perm(h@.addr)) != 0,
        ensures
            final(self).tokens == old(self).tokens,
            old(self).core.full() ==> final(self).core.full(),
            final(self).core.safe_wf(),
            final(self).core.recounted(old(self).core),
            forall|b: usize|
                b != h@.addr ==> #[trigger] final(self).core.perms@[b] == old(self).core.perms@[b],
            final(self).dummy == old(self).dummy,
            final(self).worker == old(self).worker,
            final(self).stats == old(self).stats,
    {
        let ghost s0 = self.core;
        let n = self.core.prefix_first_get(h);
        proof {
            s0.lemma_shape(h@.addr);
        }
        let m = if up {
            n + 1
        } else {
            n - 1
        };
        self.core.prefix_first_put(h, m);
        proof {
            let c = self.core;
            assert forall|b: usize| #[trigger] s0.dom().contains(b) implies {
                &&& c.perm(b).rc == s0.perm(b).rc
                &&& c.perm(b).meta == s0.perm(b).meta
                &&& c.perm(b).slots == s0.perm(b).slots
                &&& prefix_recounted(c.perm(b), s0.perm(b), s0.rc_of(b) == 0)
            } by {
                if b != h@.addr {
                    assert(c.perms@[b] == s0.perms@[b]);
                    s0.lemma_shape(b);
                }
            }
            if s0.full() {
                c.lemma_recounted(s0);
            }
            c.lemma_recounted_values(s0);
            c.lemma_safe_values(s0);
        }
    }

    /// `dup` の検査。数える参照なら真を返す。不死の物体の数は、呼ぶ側が rc を見た後で `count_immortal` で増やす。
    #[inline]
    pub(super) fn checked_dup(&self, h: *mut Header) -> (r: Result<bool, HeapError>)
        requires
            self.core.sound(),
            self.core.checked@ == M::CHECKED,
            self.core.dom().contains(h@.addr),
            h == self.core.block(h@.addr),
        ensures
            r == Err::<bool, HeapError>(HeapError::UseAfterFree) ==> !(self.core.state(h@.addr)
                == BlockState::Live || self.core.state(h@.addr) == BlockState::Building),
            r == Err::<bool, HeapError>(HeapError::LinDup) ==> self.core.lin(h@.addr),
            r matches Err(HeapError::WrongOwner { .. }) ==> self.core.rc_of(h@.addr) > 0 && p_owner(
                self.core.perm(h@.addr),
            ) != self.worker,
            r is Err ==> r == Err::<bool, HeapError>(HeapError::UseAfterFree) || r == Err::<
                bool,
                HeapError,
            >(HeapError::LinDup) || r matches Err(HeapError::WrongOwner { .. }),
            M::CHECKED && r is Ok ==> self.core.state(h@.addr) == BlockState::Live
                || self.core.state(h@.addr) == BlockState::Building,
            !M::CHECKED ==> r == Ok::<bool, HeapError>(false),
            M::CHECKED && r is Ok ==> self.core.perm(h@.addr).prefix is Some,
            r matches Ok(c) ==> (M::CHECKED && self.core.rc_of(h@.addr) != 0 ==> c),
            r matches Ok(c) ==> (M::CHECKED && self.core.rc_of(h@.addr) == 0 ==> {
                &&& c == (p_owner(self.core.perm(h@.addr)) != UNCOUNTED)
                &&& c ==> p_owner(self.core.perm(h@.addr)) < UNCOUNTED - 1
            }),
    {
        if !M::CHECKED {
            return Ok(false);
        }
        proof {
            self.core.lemma_code_state(h@.addr);
        }
        self.checked_live(h)?;
        if self.core.meta_get(h).lin() {
            return Err(HeapError::LinDup);
        }
        let rc = self.core.rc_get(h);
        if rc == 0 {
            return Ok(self.immortal_counted(h));
        }
        self.checked_owner(h, rc)?;
        Ok(true)
    }

    /// `decref` の検査。数える参照なら真を返す。不死の物体の数は、呼ぶ側が rc を見た後で `count_immortal` で減らす。
    #[inline]
    pub(super) fn checked_decref(&self, h: *mut Header) -> (r: Result<bool, HeapError>)
        requires
            self.core.sound(),
            self.core.checked@ == M::CHECKED,
            self.core.dom().contains(h@.addr),
            h == self.core.block(h@.addr),
        ensures
            r is Err ==> chain_err(r) || r == Err::<bool, HeapError>(HeapError::UseAfterFree) && !(
            self.core.state(h@.addr) == BlockState::Live || self.core.state(h@.addr)
                == BlockState::Building),
            M::CHECKED && r is Ok ==> self.core.state(h@.addr) == BlockState::Live
                || self.core.state(h@.addr) == BlockState::Building,
            !M::CHECKED ==> r == Ok::<bool, HeapError>(false),
            M::CHECKED && r is Ok ==> self.core.perm(h@.addr).prefix is Some,
            r matches Ok(c) ==> (M::CHECKED && self.core.rc_of(h@.addr) != 0 ==> c),
            r matches Ok(c) ==> (M::CHECKED && self.core.rc_of(h@.addr) == 0 ==> {
                &&& c == (p_owner(self.core.perm(h@.addr)) != UNCOUNTED)
                &&& p_owner(self.core.perm(h@.addr)) != 0
            }),
    {
        if !M::CHECKED {
            return Ok(false);
        }
        proof {
            self.core.lemma_code_state(h@.addr);
        }
        self.checked_live(h)?;
        let rc = self.core.rc_get(h);
        if rc == 0 {
            let n = self.core.prefix_first_get(h);
            if n == 0 {
                return Err(HeapError::ImmortalOverRelease);
            }
            return Ok(n != UNCOUNTED);
        }
        if rc < 0 && self.core.meta_get(h).lin() {
            return Err(HeapError::LinShared);
        }
        self.checked_owner(h, rc)?;
        Ok(true)
    }
}

// 数が UNCOUNTED に届くと、その物体はダミーと同じく数えないものに変わってしまう。2^32 - 1 回の取得は正しいプログラムでは
// 起きないので、新しい誤りの種類を足さずに止める (信頼の一覧の T5)。状態を変える前に呼ぶ (T7)
#[verifier::external_body]
#[cold]
fn too_many_acquires() -> ! {
    panic!("an immortal object is acquired 2^32 - 1 times")
}

} // verus!

const _: () = assert!(PREFIX_WORDS == size_of::<Prefix>().div_ceil(8));
const _: () = assert!(UNCOUNTED == u32::MAX);

/// # Safety
/// `h` は検査モードのヒープが確保したブロックのヘッダを指す。
#[cfg(test)]
pub(crate) unsafe fn prefix<'a>(h: NonNull<Header>) -> &'a Prefix {
    unsafe {
        &*h.as_ptr()
            .cast::<u8>()
            .sub(PREFIX_WORDS * 8)
            .cast::<Prefix>()
    }
}

verus! {

/// `release` が名指す形。データの物体で、tag と走査する語の数が名指しのとおりである
pub open spec fn named_layout(meta: Meta, scanned: usize, tag: u16, keep_len: usize) -> bool {
    meta.kind_spec() == Kind::Data && meta.tag_spec() == tag && scanned == keep_len
}

/// 箱が持つ `lin` ビットを、新しい `meta` が落とす
pub open spec fn drops_lin(boxed: Meta, new: Meta) -> bool {
    boxed.lin_spec() && !new.lin_spec()
}

/// `release` が名指す形の照らし合わせ。種類の比較に `==` でなく `matches!` を使うのは、Verus が導出した `PartialEq` に
/// 仕様を与えないためである。
pub(crate) fn layout_check(meta: Meta, scanned: usize, tag: u16, keep_len: usize) -> (r: Result<
    (),
    HeapError,
>)
    ensures
        r is Ok <==> named_layout(meta, scanned, tag, keep_len),
        r is Err ==> r == Err::<(), HeapError>(HeapError::WrongLayout),
{
    if matches!(meta.kind(), Kind::Data) && meta.tag() == tag && scanned == keep_len {
        Ok(())
    } else {
        Err(HeapError::WrongLayout)
    }
}

/// `reuse_in_place` の照らし合わせ (docs/spec/object-model.md の「`lin` ビット」)。`lin` ビットは、箱が持ち新しい `meta` が
/// 持たないときだけ誤りにする。ビットが欠けるのは安全の側だからである。
pub(crate) fn reuse_in_place_check(boxed: Meta, new: Meta) -> (r: Result<(), HeapError>)
    ensures
        !same_fields(boxed, new) ==> r == Err::<(), HeapError>(HeapError::WrongMeta),
        same_fields(boxed, new) && drops_lin(boxed, new) ==> r == Err::<(), HeapError>(
            HeapError::LinBitKept,
        ),
        same_fields(boxed, new) && !drops_lin(boxed, new) ==> r is Ok,
{
    if !boxed.same_shape(new) {
        return Err(HeapError::WrongMeta);
    }
    if boxed.lin() && !new.lin() {
        return Err(HeapError::LinBitKept);
    }
    Ok(())
}

} // verus!
