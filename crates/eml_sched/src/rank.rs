//! runのlockの順位 (docs/spec/runtime.mdの「lockの順位」)。スケジューラのlockはどれもここで取る。順位2は登録表のほか
//! のすべてのlockで、ほかの順位2のlockを持たないときにだけ取る。デバッグのビルドでは、このスレッドが持つものをスレッ
//! ドローカルな値が記録し、表明が違反を止める。リリースのビルドは何も記録しない。

use std::cell::Cell;
use std::ops::{Deref, DerefMut};
use std::sync::{Mutex, MutexGuard, PoisonError};

thread_local! {
    /// このスレッドが順位2のlockを持つか。デバッグのビルドでだけ読み書きする。
    static INNER: Cell<bool> = const { Cell::new(false) };
}

/// 順位2のlock。この下でpanicしたスレッドは、状態を1回の代入が残すとおりに残したので、毒の付いたlockもそのまま取る。
pub(crate) struct Ranked<T>(Mutex<T>);

/// 持っている順位2のlock。
pub(crate) struct Held<'a, T> {
    guard: MutexGuard<'a, T>,
}

impl<T> Ranked<T> {
    pub(crate) fn new(value: T) -> Ranked<T> {
        Ranked(Mutex::new(value))
    }

    pub(crate) fn hold(&self) -> Held<'_, T> {
        if cfg!(debug_assertions) {
            INNER.with(|inner| {
                assert!(!inner.get(), "a rank-2 lock taken under another");
                inner.set(true);
            });
        }
        Held {
            guard: self.0.lock().unwrap_or_else(PoisonError::into_inner),
        }
    }
}

impl<T> Drop for Held<'_, T> {
    fn drop(&mut self) {
        if cfg!(debug_assertions) {
            INNER.with(|inner| inner.set(false));
        }
    }
}

impl<T> Deref for Held<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.guard
    }
}

impl<T> DerefMut for Held<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.guard
    }
}

/// このスレッドがrunのlockを持たないことを表明する。`Policy::at`はそのときにだけ動く (docs/spec/runtime.mdの「フッ
/// ク」)。
pub(crate) fn unlocked() {
    if cfg!(debug_assertions) {
        INNER.with(|inner| assert!(!inner.get(), "a hook reached under a lock"));
    }
}

#[cfg(test)]
mod tests {
    use super::{Ranked, unlocked};

    #[test]
    #[cfg_attr(not(debug_assertions), ignore = "debug-build assertion")]
    #[should_panic(expected = "a rank-2 lock taken under another")]
    fn two_inner_locks_panic() {
        let a = Ranked::new(0);
        let b = Ranked::new(0);
        let _a = a.hold();
        let _b = b.hold();
    }

    #[test]
    #[cfg_attr(not(debug_assertions), ignore = "debug-build assertion")]
    #[should_panic(expected = "a hook reached under a lock")]
    fn at_under_a_lock_panics() {
        let a = Ranked::new(0);
        let _a = a.hold();
        unlocked();
    }

    #[test]
    fn a_released_lock_lets_the_next_one_be_taken() {
        let a = Ranked::new(1);
        let b = Ranked::new(2);
        let first = *a.hold();
        let second = *b.hold();
        assert_eq!(first + second, 3);
        unlocked();
    }
}
