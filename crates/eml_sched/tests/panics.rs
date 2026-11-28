//! runの中のpanicは値として戻り、`Retired`もすべて戻る (docs/spec/runtime.mdの「runの手順」)。

// SAFETY: このファイルの`unsafe`の呼び出しは、`Watching::on_worker`が`Scripts::on_worker`に任せる呼び出しだけである。
// `sequence::run`が渡した`Env`をそのスレッドで渡し、受け取ったcallbackは1回だけ包む。
// これらのテストの閉包はヒープの操作の前にpanicするので、戻ってくる`Ctx`は落としてよい
// (docs/spec/runtime.mdの「runの手順」)。

use std::sync::Mutex;

use eml_sched::error::RunFault;
use eml_sched::exec::{Env, Exec, Halt, Place, Workers};
use eml_sched::run::{Cause, Stopped};

use crate::support::{control, one, run};

fn panicked() -> Option<Stopped> {
    Some(Stopped {
        worker: 0,
        cause: Cause::Fault(RunFault::Internal("a worker thread panicked")),
    })
}

/// `W`に任せ、runのcallbackが返したものを記録するfactory。
struct Watching<W> {
    inner: W,
    seen: Mutex<Vec<Result<(), Halt>>>,
}

impl<W: Workers> Workers for Watching<W> {
    type Exec = W::Exec;
    type Retired = W::Retired;

    fn policy(&self, place: Place) -> <W::Exec as Exec>::P {
        self.inner.policy(place)
    }

    unsafe fn on_worker(
        &self,
        env: Env<'_, <W::Exec as Exec>::P>,
        run: impl FnOnce(*mut W::Exec) -> Result<(), Halt>,
    ) -> W::Retired {
        // SAFETY: このファイルの頭の約束による。
        unsafe {
            self.inner.on_worker(env, |this| {
                let result = run(this);
                self.seen.lock().unwrap().push(result);
                result
            })
        }
    }
}

#[test]
fn a_panic_in_an_item_is_reported_as_a_value() {
    let scripts = Watching {
        inner: one(|_, _, _| panic!("an item panics")),
        seen: Mutex::new(Vec::new()),
    };
    let control = control(1);
    let outcome = run(&scripts, &control);
    assert_eq!(outcome.stop, panicked());
    assert_eq!(*scripts.seen.lock().unwrap(), [Err(Halt::Panicked)]);
    assert_eq!(outcome.retired.len(), 1);
    assert_eq!(outcome.retired[0].place, Place::Worker(0));
    assert_eq!(control.worker_bits(0) & eml_rt::Control::END, 0);
}

#[test]
fn a_panicked_halt_from_enter_is_passed_to_the_factory() {
    let scripts = Watching {
        inner: one(|_, _, _| Err(Halt::Panicked)),
        seen: Mutex::new(Vec::new()),
    };
    let outcome = run(&scripts, &control(1));
    assert_eq!(outcome.stop, panicked());
    assert_eq!(*scripts.seen.lock().unwrap(), [Err(Halt::Panicked)]);
    assert_eq!(outcome.retired.len(), 1);
}
