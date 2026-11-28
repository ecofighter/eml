//! runのlockの順位 (docs/spec/runtime.mdの「lockの順位」)。スケジューラのlockはどれもここで取る。順位1は登録表で、
//! 順位2はほかのすべてのlockである。順位2のlockは、ほかの順位2のlockを持たないときにだけ取り、持つ間は登録表を取らな
//! い。デバッグのビルドでは、このスレッドが持つものをスレッドローカルな値が記録し、表明が違反を止める。リリースのビル
//! ドは何も記録しない。

use std::cell::Cell;
use std::ops::{Deref, DerefMut};
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::cells::{Registry, Shared};

thread_local! {
    /// このスレッドが順位2のlockを持つか。デバッグのビルドでだけ読み書きする。
    static INNER: Cell<bool> = const { Cell::new(false) };
    /// このスレッドが登録表を持つことの印の深さ。スケジューラのファイルの`cells`の呼び出しは、その中で`cells`が呼ぶ
    /// `Mailer`の印と入れ子になり、エンジンの行の呼び出しは`Mailer`でだけ印を付けるので、真偽ではなく深さで数える。
    /// デバッグのビルドでだけ読み書きする。
    static DEPTH: Cell<u32> = const { Cell::new(0) };
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
        let held = INNER.with(Cell::get) || DEPTH.with(Cell::get) > 0;
        assert!(!held, "a hook reached under a lock");
    }
}

/// 登録表を取ってよいことを表明する。順位2のlockの下でも、登録表の下でも取らない。登録表のlockは入れ子にできない。
fn may_take_the_registry() {
    if cfg!(debug_assertions) {
        assert!(
            !INNER.with(Cell::get),
            "the registry taken under a rank-2 lock"
        );
        assert_eq!(DEPTH.with(Cell::get), 0, "the registry taken twice");
    }
}

/// 登録表を持つことの印。落とすと外れる。
pub(crate) struct Mark(());

impl Mark {
    fn raise() -> Mark {
        if cfg!(debug_assertions) {
            DEPTH.with(|depth| depth.set(depth.get() + 1));
        }
        Mark(())
    }
}

impl Drop for Mark {
    fn drop(&mut self) {
        if cfg!(debug_assertions) {
            DEPTH.with(|depth| depth.set(depth.get() - 1));
        }
    }
}

/// スケジューラのファイルが直接取った登録表。
pub(crate) struct Locked<'a> {
    // 欄は宣言の順に落ちるので、lockを放してから印を外す
    guard: MutexGuard<'a, Registry>,
    _mark: Mark,
}

impl Deref for Locked<'_> {
    type Target = Registry;
    fn deref(&self) -> &Registry {
        &self.guard
    }
}

impl DerefMut for Locked<'_> {
    fn deref_mut(&mut self) -> &mut Registry {
        &mut self.guard
    }
}

/// 登録表をlockする。
pub(crate) fn registry(shared: &Shared) -> Locked<'_> {
    may_take_the_registry();
    let mark = Mark::raise();
    Locked {
        guard: shared.lock(),
        _mark: mark,
    }
}

/// `Shared`を取る`cells`の関数の呼び出し`f`。その関数は中で登録表をlockするので、登録表を取ることに数える。
pub(crate) fn cells<R>(f: impl FnOnce() -> R) -> R {
    may_take_the_registry();
    let _mark = Mark::raise();
    f()
}

/// `cells`が登録表を持ったまま呼ぶ`Mailer`のメソッドが付ける印。
pub(crate) fn under_registry() -> Mark {
    Mark::raise()
}

#[cfg(test)]
mod tests {
    use super::{DEPTH, Ranked, cells, registry, under_registry, unlocked};
    use crate::cells::{Registry, Shared};

    #[test]
    #[cfg_attr(not(debug_assertions), ignore = "debug-build assertion")]
    #[should_panic(expected = "the registry taken under a rank-2 lock")]
    fn an_inversion_panics_in_a_debug_build() {
        let shared = Shared::new(Registry::new(Vec::new(), 1));
        let a = Ranked::new(0);
        let _a = a.hold();
        let _registry = registry(&shared);
    }

    #[test]
    #[cfg_attr(not(debug_assertions), ignore = "debug-build assertion")]
    #[should_panic(expected = "the registry taken under a rank-2 lock")]
    fn a_cells_call_under_an_inner_lock_panics() {
        let a = Ranked::new(0);
        let _a = a.hold();
        cells(|| ());
    }

    /// `rank::cells`の中で`cells`が`Mailer`を呼ぶと、印は入れ子になる。内側の印が外れても、登録表は外側の呼び出しが
    /// 終わるまで持っていることになる。
    #[test]
    #[cfg_attr(not(debug_assertions), ignore = "debug-build assertion")]
    fn a_nested_mark_stays_until_the_outer_call_ends() {
        let depth = || DEPTH.with(|depth| depth.get());
        cells(|| {
            assert_eq!(depth(), 1);
            {
                let _inner = under_registry();
                assert_eq!(depth(), 2);
            }
            assert_eq!(depth(), 1);
            let inner = Ranked::new(0);
            assert_eq!(*inner.hold(), 0);
            assert!(std::panic::catch_unwind(unlocked).is_err());
        });
        assert_eq!(depth(), 0);
        unlocked();
    }

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
