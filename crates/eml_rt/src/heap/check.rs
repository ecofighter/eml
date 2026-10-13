//! 検査モードの前置きと検査 (docs/superpowers/specs/2026-10-13-s9a-eml-rt-core-design.md の「前置き」と「報告する
//! 誤り」)。前置きは検査モードのブロックにだけあるので、前置きを読む関数は `M::CHECKED` のときだけ働く。

use std::ptr::NonNull;

use super::Heap;
use crate::error::HeapError;
use crate::meta::{self, HEADER_WORDS, Header};
use crate::mode::Mode;
use crate::sync::{AtomicU32, Ordering};

#[repr(C)]
pub(crate) struct Prefix {
    /// 普通の物体では所有者 ID、`alloc_immortal` の物体では外に出ている参照の数 (spec の「不死の物体の数え方」)。
    pub(crate) first: AtomicU32,
    /// 下位2ビットが状態、残りが世代の番号。
    state: AtomicU32,
}

pub(crate) const PREFIX_WORDS: usize = size_of::<Prefix>().div_ceil(8);
/// `first` のこの値の不死の物体 (ダミー) は、数を変えも確かめもしない。
pub(crate) const UNCOUNTED: u32 = u32::MAX;

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

/// # Safety
/// `h` は検査モードのヒープが今確保したブロックのヘッダを指す。
pub(crate) unsafe fn init_prefix(h: NonNull<Header>, first: u32) {
    let prefix = Prefix {
        first: AtomicU32::new(first),
        state: AtomicU32::new(State::Alive as u32),
    };
    unsafe {
        h.as_ptr()
            .cast::<u8>()
            .sub(PREFIX_WORDS * 8)
            .cast::<Prefix>()
            .write(prefix)
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
        first.store(n + 1, Ordering::Relaxed);
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

    /// 正の rc の物体は、今の worker が持ち主でなければならない (spec の「論理的な worker ID」)。
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

    pub(super) unsafe fn check_bounds(
        &self,
        h: NonNull<Header>,
        i: usize,
    ) -> Result<(), HeapError> {
        if !M::CHECKED {
            return Ok(());
        }
        let capacity = unsafe { self.pages.block_words(h) } - PREFIX_WORDS - HEADER_WORDS;
        if i < capacity {
            Ok(())
        } else {
            Err(HeapError::OutOfBounds)
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
