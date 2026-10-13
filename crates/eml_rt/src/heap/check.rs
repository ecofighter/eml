//! 検査モードの前置きと検査 (docs/spec/object-model.md の「前置き」と「報告する
//! 誤り」)。前置きは検査モードのブロックにだけあるので、前置きを読む関数は `M::CHECKED` のときだけ働く。

use std::ptr::NonNull;

use super::Heap;
use crate::cell::{u32_load, u32_store};
use crate::error::HeapError;
#[cfg(verus_only)]
use crate::meta::field_ptr;
#[cfg(verus_only)]
use crate::meta::same_fields;
use crate::meta::{self, Header, Kind, Meta, PrefixPerm, prefix_first_ptr, prefix_state_ptr};
use crate::mode::Mode;
use std::sync::atomic::{AtomicU32, Ordering};
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
{
    let p = prefix_state_ptr(h);
    let bits = u32_load(p, Tracked(&perm.state));
    let generation = (bits >> 2u32).wrapping_add(1);
    u32_store(p, Tracked(&mut perm.state), (generation << 2u32) | FREED_STATE);
}

// 検査モードの検査。まだ証明していない (信頼の一覧のテストの「未証明」)。速いモードでは何も確かめない
impl<M: Mode> Heap<M> {
    #[verifier::external_body]
    #[inline]
    pub(super) fn checked_live(&self, h: *mut Header) -> (r: Result<(), HeapError>)
        ensures
            !M::CHECKED ==> r is Ok,
    {
        unsafe { self.check_live(NonNull::new_unchecked(h)) }
    }

    #[verifier::external_body]
    #[inline]
    pub(super) fn checked_owner(&self, h: *mut Header, rc: i32) -> (r: Result<(), HeapError>)
        ensures
            !M::CHECKED ==> r is Ok,
    {
        unsafe { self.check_owner(NonNull::new_unchecked(h), rc) }
    }

    #[verifier::external_body]
    #[inline]
    pub(super) fn checked_dup(&self, h: *mut Header) -> (r: Result<bool, HeapError>)
        ensures
            !M::CHECKED ==> r == Ok::<bool, HeapError>(false),
    {
        unsafe { self.check_dup(NonNull::new_unchecked(h)) }
    }

    /// 検査モードで、使い回す箱の前置きを「生存」に戻す。
    #[verifier::external_body]
    #[inline]
    pub(super) fn checked_set_alive(&self, h: *mut Header) {
        let p = unsafe { prefix(NonNull::new_unchecked(h)) };
        p.set_state(State::Alive, false)
    }

    #[verifier::external_body]
    #[inline]
    pub(super) fn checked_count_immortal(&self, h: *mut Header) -> (r: Result<bool, HeapError>)
        ensures
            !M::CHECKED ==> r == Ok::<bool, HeapError>(false),
    {
        unsafe { count_immortal(NonNull::new_unchecked(h), true) }
    }

    #[verifier::external_body]
    #[inline]
    pub(super) fn checked_decref(&self, h: *mut Header) -> (r: Result<bool, HeapError>)
        ensures
            !M::CHECKED ==> r == Ok::<bool, HeapError>(false),
    {
        unsafe { self.check_decref(NonNull::new_unchecked(h)) }
    }
}

} // verus!

const _: () = assert!(FREED_STATE == State::Freed as u32);
const _: () = assert!(PREFIX_WORDS == size_of::<Prefix>().div_ceil(8));
const _: () = assert!(UNCOUNTED == u32::MAX);
const _: () = assert!(ALIVE_STATE == State::Alive as u32);

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum State {
    Alive = 1,
    Token = 2,
    Freed = 3,
}

impl Prefix {
    pub(crate) fn state(&self) -> State {
        match self.state.load(Ordering::Relaxed) & 3 {
            1 => State::Alive,
            2 => State::Token,
            3 => State::Freed,
            bits => unreachable!("an unknown block state {bits}"),
        }
    }

    pub(crate) fn generation(&self) -> u32 {
        self.state.load(Ordering::Relaxed) >> 2
    }

    /// 状態を替える。`advance` なら世代の番号を1つ進める (`into_token` と解放)。
    pub(crate) fn set_state(&self, state: State, advance: bool) {
        let generation = self.generation().wrapping_add(u32::from(advance));
        self.state
            .store((generation << 2) | state as u32, Ordering::Relaxed);
    }
}

/// # Safety
/// `h` は検査モードのヒープが確保したブロックのヘッダを指す。
pub(crate) unsafe fn prefix<'a>(h: NonNull<Header>) -> &'a Prefix {
    unsafe {
        &*h.as_ptr()
            .cast::<u8>()
            .sub(PREFIX_WORDS * 8)
            .cast::<Prefix>()
    }
}

/// 不死の物体の外に出ている参照の数を1つ増やすか減らす。数えた参照なら真を返す。
///
/// # Safety
/// `h` は検査モードの生きている不死の物体を指す。
pub(crate) unsafe fn count_immortal(h: NonNull<Header>, up: bool) -> Result<bool, HeapError> {
    let first = &unsafe { prefix(h) }.first;
    let n = first.load(Ordering::Relaxed);
    if n == UNCOUNTED {
        return Ok(false);
    }
    if up {
        // 数が UNCOUNTED に届くと、その物体はダミーと同じく数えないものに変わってしまう。2^32 - 1 回の取得は正しい
        // プログラムでは起きないので、新しい誤りの種類を足さずに panic にする
        let up = n
            .checked_add(1)
            .filter(|&m| m != UNCOUNTED)
            .expect("an immortal object is acquired 2^32 - 1 times");
        first.store(up, Ordering::Relaxed);
    } else if n == 0 {
        return Err(HeapError::ImmortalOverRelease);
    } else {
        first.store(n - 1, Ordering::Relaxed);
    }
    Ok(true)
}

impl<M: Mode> Heap<M> {
    /// 前置きの状態が生存かを確かめる。解放した共有の物体は rc が 0 になっていて不死の物体と区別できないので、rc を
    /// 見る前に呼ぶ。
    pub(super) unsafe fn check_live(&self, h: NonNull<Header>) -> Result<(), HeapError> {
        if !M::CHECKED || unsafe { prefix(h) }.state() == State::Alive {
            Ok(())
        } else {
            Err(HeapError::UseAfterFree)
        }
    }

    /// 正の rc の物体は、今の worker が持ち主でなければならない (docs/spec/object-model.md の「論理的な worker ID」)。
    pub(super) unsafe fn check_owner(&self, h: NonNull<Header>, rc: i32) -> Result<(), HeapError> {
        if !M::CHECKED || rc <= 0 {
            return Ok(());
        }
        let owner = unsafe { prefix(h) }.first.load(Ordering::Relaxed);
        if owner == self.worker {
            Ok(())
        } else {
            Err(HeapError::WrongOwner {
                owner,
                worker: self.worker,
            })
        }
    }

    /// `dup` の検査。数える参照なら真を返す。
    pub(super) unsafe fn check_dup(&self, h: NonNull<Header>) -> Result<bool, HeapError> {
        if !M::CHECKED {
            return Ok(false);
        }
        unsafe { self.check_live(h)? };
        if unsafe { meta::meta(h) }.lin() {
            return Err(HeapError::LinDup);
        }
        let rc = unsafe { meta::rc(h) }.load(Ordering::Relaxed);
        if rc == 0 {
            return unsafe { count_immortal(h, true) };
        }
        unsafe { self.check_owner(h, rc)? };
        Ok(true)
    }

    /// `decref` の検査。数える参照なら真を返す。
    pub(super) unsafe fn check_decref(&self, h: NonNull<Header>) -> Result<bool, HeapError> {
        if !M::CHECKED {
            return Ok(false);
        }
        unsafe { self.check_live(h)? };
        let rc = unsafe { meta::rc(h) }.load(Ordering::Relaxed);
        if rc == 0 {
            return unsafe { count_immortal(h, false) };
        }
        if rc < 0 && unsafe { meta::meta(h) }.lin() {
            return Err(HeapError::LinShared);
        }
        unsafe { self.check_owner(h, rc)? };
        Ok(true)
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
