//! runのただ1つの手順。workerを始め、どの道でもrunの終わりを受け持ち、ただ1つの`Control::clear_end`を持つ
//! (docs/spec/runtime.mdの「runの手順」)。

use std::io::{self, Write};
use std::mem::{self, ManuallyDrop};
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use eml_rt::{Control, HostRef, Owners};

use crate::config::{RunConfig, Seat};
use crate::core::WorkerCore;
use crate::drive::drive;
use crate::error::RunFault;
use crate::exec::{Env, Exec, Halt, Place, Workers};
use crate::rank::Ranked;
use crate::run::{Cause, PANICKED, Run, Stopped};

/// runを始めるときに渡すもの。runは`config`の`control`と`seat`を読む。workerの数は`config.control.limits().workers`
/// である。hostは借りるので、それを落とすのはrunの中ではない。
pub struct Start<'s> {
    /// runのすべての`Ctx`の所有者の表。
    pub owners: &'s Owners,
    pub host: &'s HostRef,
    pub config: &'s RunConfig,
}

/// workerの`Retired`と、それが動いた場所。
pub struct Retiree<R> {
    pub place: Place,
    pub retired: R,
}

/// runの終わり方。`stop`は最初に記録した止まり、`exit`は`Control::exit_code`である。`retired`は、factoryを動かしたす
/// べてのworkerを持ち、ループがpanicしたworkerも含む。そのヒープは操作の途中かもしれないので、呼ぶ側はその中の何もた
/// どらず、確かめない。
pub struct Outcome<R> {
    pub stop: Option<Stopped>,
    pub exit: Option<i64>,
    pub retired: Vec<Retiree<R>>,
}

/// runの確定した終わり。
struct Settled {
    stop: Option<Stopped>,
    exit: Option<i64>,
}

/// runのcallbackと各workerの根元が分け合うもの。`run`のフレームの中にある。
struct Shell<'s> {
    control: &'s Arc<Control>,
    run: Arc<Run>,
    /// 呼ぶスレッドのcallbackが始まったか。
    entered: AtomicBool,
    settled: Ranked<Option<Settled>>,
}

/// runを終わりまで動かす。今は合わせた席で1つのworker、worker 0を動かし、呼ぶスレッドがworker 0である。workerの数がほ
/// かの数のrunと、分かれた席のrunは、何も始めずに内部の誤りを返す。
///
/// # Safety
/// `W`は`Exec`と`Workers`の約束を守る (docs/spec/runtime.mdの「信頼の一覧」のSM-5とSM-9)。runのエンジンが`WorkerCore`
/// を通して置く語はどれも`cells`のC2を守り、`WorkerCore`に対するどの呼び出しもスケジューラの不変条件を守る。
/// `start.owners`は、runのすべての`Ctx`の所有者の表である。このrunが動く間、ほかのrunは`start.config.control`を使わな
/// い。
pub unsafe fn run<W: Workers>(workers: &W, start: Start<'_>) -> Outcome<W::Retired> {
    let Start {
        owners,
        host,
        config,
    } = start;
    let control = &config.control;
    if control.limits().workers != 1 || config.seat != Seat::Merged {
        return Outcome {
            stop: Some(Stopped {
                worker: 0,
                cause: Cause::Fault(RunFault::Internal(
                    "this run sequence runs one worker in the merged seat",
                )),
            }),
            exit: None,
            retired: Vec::new(),
        };
    }
    control.begin_run();
    let shell = Shell {
        control,
        run: Arc::new(Run::new()),
        entered: AtomicBool::new(false),
        settled: Ranked::new(None),
    };
    // SAFETY: この関数の`# Safety`による。ここは呼ぶスレッドで、合わせた席ではworker 0である。
    let retired = unsafe { own_worker(workers, &shell, owners, host, 0) };
    let _ = catch(|| {
        if !shell.entered.load(Ordering::Relaxed) {
            shell.left_without_running(0);
        }
    });
    let settled = catch(|| shell.settled());
    outcome(
        settled,
        vec![Retiree {
            place: Place::Worker(0),
            retired: ManuallyDrop::into_inner(retired),
        }],
    )
}

/// worker `k`のfactoryをこのスレッドで動かす。その`Retired`は受け取る式から`ManuallyDrop`の中にあるので、巻き戻しはそ
/// れをリークする。factoryから出る巻き戻しはプロセスを中止する。
///
/// # Safety
/// `run`と同じ。ここはworker `k`のスレッドである。
unsafe fn own_worker<'s, W: Workers>(
    workers: &W,
    shell: &'s Shell<'s>,
    owners: &Owners,
    host: &HostRef,
    k: u32,
) -> ManuallyDrop<W::Retired> {
    let place = Place::Worker(k);
    // SAFETY: `this`はfactoryが`run`に渡すポインタで (SM-9)、`run`の`# Safety`が成り立つ。
    let run = |this| unsafe { callback(shell, k, this) };
    let core = || {
        WorkerCore::new(
            k,
            workers.policy(place),
            shell.control.clone(),
            shell.run.clone(),
        )
    };
    match catch(|| {
        // SAFETY: `run`の`# Safety`による (SM-9)。ここはworker `k`のスレッドで、`sequence::run`はfactoryを1回呼ぶ。
        ManuallyDrop::new(unsafe {
            workers.on_worker(Env::new(place, shell.control, owners, host, core()), run)
        })
    }) {
        Some(retired) => retired,
        None => set_up_panicked(),
    }
}

/// worker `k`の`run`のcallback。各段階は自分のcatchの下にあるので、どれかのpanicは記録され、後の段階もそのまま動く。
/// 段階は、ループ、ループの終わり、呼ぶスレッドでは`finish`である。
///
/// # Safety
/// `this`は、factoryがworker `k`のスレッドで`run`に渡したポインタで、`run`の`# Safety`が成り立つ。
unsafe fn callback<E: Exec>(shell: &Shell<'_>, k: u32, this: *mut E) -> Result<(), Halt> {
    let ran = catch(|| {
        shell.entered.store(true, Ordering::Relaxed);
        // SAFETY: この関数の`# Safety`による (SM-5)。
        unsafe { drive(this) }
    });
    let ended = catch(|| shell.end_of_loop(k, ran.is_none()));
    let finished = catch(|| shell.finish(k, ended != Some(Some(()))));
    catch(|| match (ran, ended, finished) {
        (Some(ran), Some(Some(())), Some(Some(()))) => ran,
        _ => Err(Halt::Panicked),
    })
    .unwrap_or(Err(Halt::Panicked))
}

impl Shell<'_> {
    /// runを終わらせる出来事。理由によらず、worker `k`のループが戻った。ENDは記録のcatchの後で立てるので、記録がpanic
    /// してもENDは立つ。記録がpanicしたら`None`を返す。
    fn end_of_loop(&self, k: u32, panicked: bool) -> Option<()> {
        let recorded = catch(|| {
            if panicked {
                self.run.record(k, PANICKED);
            }
        });
        self.control.end_run();
        recorded
    }

    /// worker `k`のfactoryが`run`を呼ばずに戻った。それがrunを終わらせる出来事で、`finish`はここで動く。
    fn left_without_running(&self, k: u32) {
        let recorded = catch(|| {
            self.run.record(
                k,
                Cause::Fault(RunFault::Internal("a worker left without running")),
            );
        });
        self.control.end_run();
        self.finish(k, recorded.is_none());
    }

    /// 呼ぶスレッドでrunを確定させる。最初に記録した止まりが勝ち、終了コードは`Control::exit_code`である。確定はcatch
    /// の
    /// 下で動き、ENDはcatchが何を返してもその後で下ろすので、確定の中のpanicはENDを残さない。確定がpanicしたら`None`
    /// を返す。
    fn finish(&self, k: u32, panicked: bool) -> Option<()> {
        let settled = catch(|| {
            if panicked {
                self.run.record(k, PANICKED);
            }
            let settled = Settled {
                stop: self.run.take_stop(),
                exit: self.control.exit_code(),
            };
            *self.settled.hold() = Some(settled);
        });
        self.control.clear_end();
        settled
    }

    /// 確定した終わり。`finish`がそれを書けなかったときは、止まりの記録と`Control`にまだ残っているもの。
    fn settled(&self) -> Settled {
        let stored = self.settled.hold().take();
        stored.unwrap_or_else(|| Settled {
            stop: self.run.take_stop().or(Some(Stopped {
                worker: 0,
                cause: PANICKED,
            })),
            exit: self.control.exit_code(),
        })
    }
}

fn outcome<R>(settled: Option<Settled>, retired: Vec<Retiree<R>>) -> Outcome<R> {
    let Settled { stop, exit } = settled.unwrap_or(Settled {
        stop: Some(Stopped {
            worker: 0,
            cause: PANICKED,
        }),
        exit: None,
    });
    Outcome {
        stop,
        exit,
        retired,
    }
}

/// `f`を動かし、panicしたら`None`を返す。payloadの落とし方もpanicしうるので、payloadは自分のcatchの下で落とし、その2
/// 度目のpanicのpayloadは忘れる。
fn catch<R>(f: impl FnOnce() -> R) -> Option<R> {
    // 捕まえたpanicの後でシェルが読むのは、順位の付いた2つの値 (止まりの記録と確定した終わり) とアトミックな値だけで
    // ある。2つの値は、panicしても1回の代入が残すとおりに残る
    match panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(value) => Some(value),
        Err(payload) => {
            if let Err(again) = panic::catch_unwind(AssertUnwindSafe(|| drop(payload))) {
                mem::forget(again);
            }
            None
        }
    }
}

/// factoryがループの外で巻き戻った。`Workers`の約束により、巻き戻しは`Ctx`をリークしたので、中止の前にほかのスレッド
/// が解放した記憶を読むことはない。
#[cold]
fn set_up_panicked() -> ! {
    let _ = writeln!(
        io::stderr(),
        "eml: a worker's set-up panicked outside its loop; aborting"
    );
    std::process::abort()
}
