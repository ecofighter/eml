//! 根のスコープとrunの終わり (docs/spec/runtime.mdの「根」「判定」「workerのループ」)。

// SAFETY: このファイルの`unsafe`の呼び出しは、`Scripts::lending`、項の`cells`の呼び出しと`WorkerCore::here`、テスト
// が自分の`Ctx`に行うヒープの操作である。貸す`Ctx`は、このテストのスレッドで作り、runの間はほかの何も使わない。項は、
// 自分が借りた核と`Ctx`の上で、`Here`を`cells`の関数にだけ渡し、置く語は即値だけである。例外は
// `a_boxed_closure_is_released_by_its_item`と`a_boxed_closure_left_by_a_halt_stays_counted`である。`main`はその
// `Ctx`に確保した物体1つを閉包として置き、タスクの項がそれをちょうど1回放すか、誰も放さない。

use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use eml_rt::{Checked, Control, Ctx, Word};
use eml_sched::cells::{CellRef, Runnable, WOKE, WaitToken};
use eml_sched::config::RunConfig;
use eml_sched::error::RunFault;
use eml_sched::exec::Halt;
use eml_sched::job::{EntryKind, Item, Root};
use eml_sched::policy::InOrder;
use eml_sched::run::{Cause, Stopped};
use eml_sched::script::Scripts;
use eml_sched::sequence::{self, Start};

use crate::support::{
    control, finish, host, one, open_root, run, run_with, spawn, spawn_with, started,
};

fn fault(fault: RunFault) -> Option<Stopped> {
    Some(Stopped {
        worker: 0,
        cause: Cause::Fault(fault),
    })
}

/// `main`が根のスコープを開いてタスクを1つ置き、そのタスクが終わると、スコープが閉じてrunが終わる。
#[test]
fn the_run_ends_when_the_root_scope_closes() {
    let ran = AtomicU32::new(0);
    let scripts = one(|core, ctx, item| {
        match item {
            Item::Root(Root::Main) => {
                let scope = open_root(core);
                spawn(core, ctx, scope);
            }
            Item::Entry(entry) => {
                assert_eq!(entry.kind, EntryKind::Start);
                started(core, entry.cell);
                finish(core, ctx, entry.cell);
                ran.fetch_add(1, Ordering::Relaxed);
            }
            Item::Root(Root::Result) => {}
        }
        Ok(())
    });
    let control = control(1);
    let outcome = run(&scripts, &control);
    assert_eq!(outcome.stop, None);
    assert_eq!(ran.load(Ordering::Relaxed), 1);
    assert_eq!(control.worker_bits(0) & Control::END, 0);
}

/// 結果の項は、根のスコープが閉じた後に1回だけ動く。
#[test]
fn the_result_item_runs_after_the_scope_closes() {
    let order = Mutex::new(Vec::new());
    let root = Mutex::new(None::<CellRef>);
    let scripts = one(|core, ctx, item| {
        match item {
            Item::Root(Root::Main) => {
                let scope = open_root(core);
                *root.lock().unwrap() = Some(scope);
                spawn(core, ctx, scope);
                order.lock().unwrap().push("main");
            }
            Item::Entry(entry) => {
                started(core, entry.cell);
                let scope = root.lock().unwrap().unwrap();
                assert!(!core.shared().lock().closed(scope).unwrap());
                finish(core, ctx, entry.cell);
                order.lock().unwrap().push("task");
            }
            Item::Root(Root::Result) => {
                let scope = root.lock().unwrap().unwrap();
                assert!(core.shared().lock().closed(scope).unwrap());
                order.lock().unwrap().push("result");
            }
        }
        Ok(())
    });
    let outcome = run(&scripts, &control(1));
    assert_eq!(outcome.stop, None);
    assert_eq!(*order.lock().unwrap(), ["main", "task", "result"]);
}

/// `main`が置いたタスクと、そのタスクが置いたタスクは、どちらも結果の項より先に動く。
#[test]
fn a_task_started_by_main_runs_before_the_end() {
    let order = Mutex::new(Vec::new());
    let first = Mutex::new(None::<CellRef>);
    let scripts = one(|core, ctx, item| {
        match item {
            Item::Root(Root::Main) => {
                let scope = open_root(core);
                *first.lock().unwrap() = Some(spawn(core, ctx, scope));
            }
            Item::Entry(entry) => {
                started(core, entry.cell);
                if Some(entry.cell) == *first.lock().unwrap() {
                    let scope = core.shared().lock().current_of(entry.cell).unwrap();
                    spawn(core, ctx, scope);
                    order.lock().unwrap().push("first");
                } else {
                    order.lock().unwrap().push("second");
                }
                finish(core, ctx, entry.cell);
            }
            Item::Root(Root::Result) => order.lock().unwrap().push("result"),
        }
        Ok(())
    });
    let outcome = run(&scripts, &control(1));
    assert_eq!(outcome.stop, None);
    assert_eq!(*order.lock().unwrap(), ["first", "second", "result"]);
}

/// タスクが待つタスクが終わると、待つタスクは継続と`WOKE`で再開する。
#[test]
fn a_resumed_task_gets_its_wake() {
    let k = Word::imm(41);
    let tasks = Mutex::new(Vec::<CellRef>::new());
    let resumed = Mutex::new(None);
    let scripts = one(|core, ctx, item| {
        match item {
            Item::Root(Root::Main) => {
                let scope = open_root(core);
                let a = spawn(core, ctx, scope);
                let b = spawn(core, ctx, scope);
                *tasks.lock().unwrap() = vec![a, b];
            }
            Item::Entry(entry) => {
                let [a, b] = tasks.lock().unwrap()[..] else {
                    unreachable!()
                };
                match entry.kind {
                    EntryKind::Start if entry.cell == a => {
                        started(core, a);
                        let run = core.shared().lock().park(b.encode(), a, k).unwrap();
                        assert!(run.is_none(), "b has not finished");
                    }
                    EntryKind::Start => {
                        started(core, b);
                        finish(core, ctx, b);
                    }
                    EntryKind::Resume { wake } => {
                        assert_eq!(entry.cell, a);
                        let wake = core.shared().lock().resumed(a, wake, false).unwrap();
                        *resumed.lock().unwrap() = Some((wake, entry.word));
                        finish(core, ctx, a);
                    }
                    EntryKind::ResumeIo { .. } => unreachable!("no task waits for an operation"),
                }
            }
            Item::Root(Root::Result) => {}
        }
        Ok(())
    });
    let outcome = run(&scripts, &control(1));
    assert_eq!(outcome.stop, None);
    assert_eq!(*resumed.lock().unwrap(), Some((WOKE, k)));
}

/// tokenで待つものはこのスケジューラにはないので、tokenの起こしは内部の誤りでrunを終わらせる。
#[test]
fn a_token_runnable_is_an_internal_error() {
    let after = AtomicU32::new(0);
    let scripts = one(|core, _, item| {
        match item {
            Item::Root(Root::Main) => core.schedule(Runnable::Token(WaitToken(7))),
            _ => {
                after.fetch_add(1, Ordering::Relaxed);
            }
        }
        Ok(())
    });
    let control = control(1);
    let outcome = run(&scripts, &control);
    assert_eq!(
        outcome.stop,
        fault(RunFault::Internal(
            "a token wait reached the native scheduler"
        ))
    );
    assert_eq!(after.load(Ordering::Relaxed), 0);
    assert_eq!(control.worker_bits(0) & Control::END, 0);
}

/// どの項も、空の`evv`で、yieldの途中でなく、置いた失敗なしに開く。タスクの項はタスクのIDの下で、根の項はworkerの
/// IDの下で動く。
#[test]
fn an_item_opens_with_an_empty_evidence_vector() {
    let home = Mutex::new(None::<u32>);
    let items = AtomicU32::new(0);
    let scripts = one(|core, ctx, item| {
        assert_eq!(ctx.evv_word(), Word::UNIT);
        assert_eq!(ctx.yielding(), 0);
        assert!(ctx.take_fault().is_none());
        items.fetch_add(1, Ordering::Relaxed);
        match item {
            Item::Root(root) => {
                let id = ctx.heap().worker();
                let mut home = home.lock().unwrap();
                assert_eq!(*home.get_or_insert(id), id);
                if root == Root::Main {
                    let scope = open_root(core);
                    spawn(core, ctx, scope);
                }
            }
            Item::Entry(entry) => {
                assert_eq!(ctx.heap().worker(), entry.worker);
                assert_ne!(Some(entry.worker), *home.lock().unwrap());
                started(core, entry.cell);
                finish(core, ctx, entry.cell);
            }
        }
        Ok(())
    });
    let outcome = run(&scripts, &control(1));
    assert_eq!(outcome.stop, None);
    assert_eq!(items.load(Ordering::Relaxed), 3);
}

/// 根のスコープが開いたまま、どのタスクも待っていて、待ちを終わらせるものがない。
#[test]
fn every_task_waiting_is_a_deadlock() {
    let scripts = one(|core, ctx, item| {
        match item {
            Item::Root(Root::Main) => {
                let scope = open_root(core);
                spawn(core, ctx, scope);
            }
            Item::Entry(entry) => {
                started(core, entry.cell);
                let scope = core.shared().lock().current_of(entry.cell).unwrap();
                let run = core
                    .shared()
                    .lock()
                    .park(scope.encode(), entry.cell, Word::imm(1))
                    .unwrap();
                assert!(run.is_none(), "the scope is open");
            }
            Item::Root(Root::Result) => unreachable!("the scope never closes"),
        }
        Ok(())
    });
    let control = control(1);
    let outcome = run(&scripts, &control);
    assert_eq!(outcome.stop, fault(RunFault::Deadlock));
    assert_eq!(control.worker_bits(0) & Control::END, 0);
}

/// worker 1が持つタスクは、workerが1つのrunでは置けない。知らせを送る先がないので、内部の誤りである。
#[test]
fn a_task_placed_on_another_worker_of_a_run_of_one_is_an_internal_error() {
    let entries = AtomicU32::new(0);
    let scripts = one(|core, ctx, item| {
        match item {
            Item::Root(Root::Main) => {
                let scope = open_root(core);
                let spawner = ctx.heap().worker();
                // SAFETY: このファイルの頭の約束による。
                let here = unsafe { core.here() };
                // SAFETY: このファイルの頭の約束による。閉包は即値である。
                let started = unsafe {
                    core.shared().lock().start(
                        ctx,
                        &here,
                        scope,
                        spawner,
                        1,
                        Word::UNIT,
                        &[],
                        false,
                    )
                };
                assert!(matches!(started, Ok(Some((_, None)))));
            }
            _ => {
                entries.fetch_add(1, Ordering::Relaxed);
            }
        }
        Ok(())
    });
    let outcome = run(&scripts, &control(1));
    assert_eq!(
        outcome.stop,
        fault(RunFault::Internal(
            "a notice is sent to another worker of a run of one worker"
        ))
    );
    assert_eq!(entries.load(Ordering::Relaxed), 0);
}

/// 貸す`Ctx`で動く、根のスコープにタスクを1つ置く`main`の閉包。`halt`ならタスクの項が止まる。
fn session_main(
    halt: bool,
) -> impl Fn(&mut eml_sched::core::WorkerCore<InOrder>, &mut Ctx<Checked>, Item) -> Result<(), Halt>
+ Send
+ Sync {
    move |core, ctx, item| {
        match item {
            Item::Root(Root::Main) => {
                let scope = open_root(core);
                spawn(core, ctx, scope);
            }
            Item::Entry(_) if halt => return Err(Halt::Stopped),
            Item::Entry(entry) => {
                started(core, entry.cell);
                finish(core, ctx, entry.cell);
            }
            Item::Root(Root::Result) => {}
        }
        Ok(())
    }
}

/// 1つのセッションの`Ctx`の上で、成功するrun、タスクの項で止まるrun、テストの回復、成功するrunを続ける。どのrunの後
/// も回復の後も`Ctx`は同じIDの下にあり、3つ目のrunの`main`もそのIDで動く。
#[test]
fn two_runs_on_one_ctx_with_a_failure_between() {
    let control = control(1);
    let mut ctx = Ctx::<Checked>::new(control.clone(), host());
    let first = ctx.heap().worker();
    let mains = Mutex::new(Vec::new());
    for (halt, stop) in [
        (false, None),
        (
            true,
            Some(Stopped {
                worker: 0,
                cause: Cause::Halt,
            }),
        ),
        (false, None),
    ] {
        let owners = ctx.heap().owners();
        {
            let inner = session_main(halt);
            let enter = |core: &mut eml_sched::core::WorkerCore<InOrder>,
                         ctx: &mut Ctx<Checked>,
                         item: Item| {
                if item == Item::Root(Root::Main) {
                    mains.lock().unwrap().push(ctx.heap().worker());
                }
                inner(core, ctx, item)
            };
            // SAFETY: このファイルの頭の約束による。
            let scripts = unsafe { Scripts::lending(&mut ctx, |_| InOrder, vec![Box::new(enter)]) };
            let outcome = run_with(&scripts, &control, &owners);
            assert_eq!(outcome.stop, stop);
            assert_eq!(outcome.retired.len(), 1);
            assert!(
                outcome.retired[0].retired.is_none(),
                "the lent Ctx is the caller's"
            );
        }
        assert_eq!(control.worker_bits(0) & Control::END, 0);
        assert_eq!(ctx.heap().worker(), first);
        if halt {
            // SAFETY: 根の表は空で、ほかのヒープはない。
            unsafe { ctx.recover() }.unwrap();
            assert_eq!(ctx.heap().worker(), first);
        }
    }
    assert_eq!(*mains.lock().unwrap(), [first; 3]);
}

/// タスクの項が`Halt::Stopped`で止まっても、workerはループの始めのIDに戻る。貸した`Ctx`ではセッションのIDに、作った
/// `Ctx`では0に戻る。
#[test]
fn a_halt_inside_a_task_returns_to_the_home_id() {
    let control = control(1);
    let mut ctx = Ctx::<Checked>::new(control.clone(), host());
    // SAFETY: 切り替えるのは即値の`evv`で、yieldの途中ではない。
    let session = unsafe {
        let id = ctx.heap_mut().fork_worker();
        ctx.switch_task(Word::UNIT, id, 0);
        id
    };
    let owners = ctx.heap().owners();
    {
        // SAFETY: このファイルの頭の約束による。
        let scripts =
            unsafe { Scripts::lending(&mut ctx, |_| InOrder, vec![Box::new(session_main(true))]) };
        let outcome = run_with(&scripts, &control, &owners);
        assert_eq!(
            outcome.stop,
            Some(Stopped {
                worker: 0,
                cause: Cause::Halt
            })
        );
    }
    assert_eq!(ctx.heap().worker(), session);
    let scripts = one(session_main(true));
    let outcome = run(&scripts, &control);
    assert_eq!(
        outcome.stop,
        Some(Stopped {
            worker: 0,
            cause: Cause::Halt
        })
    );
    let retired = outcome.retired.into_iter().next().unwrap().retired.unwrap();
    assert_eq!(retired.heap().worker(), 0);
}

/// 期限で眠るタスクは、待ちを終わらせる源を持つので、デッドロックではない。
#[test]
fn a_task_parked_on_a_deadline_is_not_a_deadlock() {
    let scripts = one(|core, ctx, item| {
        match item {
            Item::Root(Root::Main) => {
                let scope = open_root(core);
                spawn(core, ctx, scope);
            }
            Item::Entry(entry) => match entry.kind {
                EntryKind::Start => {
                    started(core, entry.cell);
                    let run = core
                        .shared()
                        .lock()
                        .park_time(0, 5, entry.cell, Word::imm(1))
                        .unwrap();
                    assert!(run.is_none(), "the deadline has not passed");
                }
                _ => {
                    core.shared()
                        .lock()
                        .resumed(entry.cell, WOKE, false)
                        .unwrap();
                    finish(core, ctx, entry.cell);
                }
            },
            Item::Root(Root::Result) => {}
        }
        Ok(())
    });
    let outcome = run(&scripts, &control(1));
    assert_ne!(outcome.stop, fault(RunFault::Deadlock));
}

/// `main`は自分の`Ctx`に確保した物体を閉包とするタスクを置き、タスクの項がそれをタスクのIDの下で放す。runの後、ヒープ
/// に物体は残らない。
#[test]
fn a_boxed_closure_is_released_by_its_item() {
    let scripts = one(|core, ctx, item| {
        match item {
            Item::Root(Root::Main) => {
                let scope = open_root(core);
                // SAFETY: 物体はこの項が作り、その唯一の参照を始まりの項に渡す。
                let closure = unsafe { ctx.heap_mut() }.alloc_data(0, 1, 0, false);
                spawn_with(core, ctx, scope, closure);
            }
            Item::Entry(entry) => {
                assert_eq!(ctx.heap().worker(), entry.worker);
                started(core, entry.cell);
                // SAFETY: 始まりの項は閉包への参照を1つ持ち、タスクの行と同じく、タスクのIDの下でそれを1回放す。
                unsafe { ctx.heap_mut().decref(entry.word) }.unwrap();
                finish(core, ctx, entry.cell);
            }
            Item::Root(Root::Result) => {}
        }
        Ok(())
    });
    let outcome = run(&scripts, &control(1));
    assert_eq!(outcome.stop, None);
    let mut ctx = outcome.retired.into_iter().next().unwrap().retired.unwrap();
    ctx.reclaim();
    assert!(ctx.leaks().is_empty(), "{:?}", ctx.leaks());
}

/// `main`が物体を閉包とするタスクを置いてから止まると、一度も動かなかったエントリは語に触れずに落ち、物体はヒープに
/// 数えられたまま残る。
#[test]
fn a_boxed_closure_left_by_a_halt_stays_counted() {
    let scripts = one(|core, ctx, item| {
        match item {
            Item::Root(Root::Main) => {
                let scope = open_root(core);
                // SAFETY: 物体はこの項が作り、その唯一の参照を始まりの項に渡す。
                let closure = unsafe { ctx.heap_mut() }.alloc_data(0, 1, 0, false);
                spawn_with(core, ctx, scope, closure);
                Err(Halt::Stopped)
            }
            _ => unreachable!("main halts before any task runs"),
        }
    });
    let outcome = run(&scripts, &control(1));
    assert_eq!(
        outcome.stop,
        Some(Stopped {
            worker: 0,
            cause: Cause::Halt
        })
    );
    let mut ctx = outcome.retired.into_iter().next().unwrap().retired.unwrap();
    ctx.reclaim();
    let leaks = ctx.leaks();
    assert_eq!(leaks.values().sum::<u64>(), 1, "{leaks:?}");
}

/// ループは根のスコープを、`main`が戻ったときに1回だけ読む。後の`set_root`は何も変えない。
#[test]
fn the_root_scope_is_read_once() {
    let scripts = one(|core, ctx, item| {
        match item {
            Item::Root(Root::Main) => {
                let scope = open_root(core);
                spawn(core, ctx, scope);
            }
            Item::Entry(entry) => {
                started(core, entry.cell);
                core.set_root(entry.cell);
                finish(core, ctx, entry.cell);
            }
            Item::Root(Root::Result) => {}
        }
        Ok(())
    });
    let outcome = run(&scripts, &control(1));
    assert_eq!(outcome.stop, None);
}

/// 貸す`Ctx`のworkerは、runの始めのIDで項の間を過ごし、runの後もそのIDにいる。
#[test]
fn a_lent_ctx_returns_to_the_sessions_id() {
    let control = control(1);
    let mut ctx = Ctx::<Checked>::new(control.clone(), host());
    // SAFETY: 切り替えるのは即値の`evv`で、yieldの途中ではない。
    let session = unsafe {
        let id = ctx.heap_mut().fork_worker();
        ctx.switch_task(Word::UNIT, id, 0);
        id
    };
    assert_ne!(session, 0);
    let seen = Mutex::new(Vec::new());
    let owners = ctx.heap().owners();
    {
        let enter = |core: &mut eml_sched::core::WorkerCore<InOrder>,
                     ctx: &mut Ctx<Checked>,
                     item: Item| {
            seen.lock().unwrap().push(ctx.heap().worker());
            session_main(false)(core, ctx, item)
        };
        // SAFETY: このファイルの頭の約束による。
        let scripts = unsafe { Scripts::lending(&mut ctx, |_| InOrder, vec![Box::new(enter)]) };
        let outcome = run_with(&scripts, &control, &owners);
        assert_eq!(outcome.stop, None);
    }
    let seen = seen.into_inner().unwrap();
    assert_eq!(seen.len(), 3);
    assert_eq!(seen[0], session);
    assert_ne!(seen[1], session, "the task runs under its own ID");
    assert_eq!(seen[2], session);
    assert_eq!(ctx.heap().worker(), session);
}

/// セッションの`Ctx`を貸すrunは、workerを1つだけ持つ。2つのworkerのrunは何も始めない。
#[test]
fn a_session_run_with_two_workers_does_not_start() {
    let control = control(2);
    let mut ctx = Ctx::<Checked>::new(control.clone(), host());
    let owners = ctx.heap().owners();
    let calls = AtomicU32::new(0);
    let enter = |_: &mut eml_sched::core::WorkerCore<InOrder>, _: &mut Ctx<Checked>, _: Item| {
        calls.fetch_add(1, Ordering::Relaxed);
        Ok(())
    };
    // SAFETY: このファイルの頭の約束による。
    let scripts = unsafe {
        Scripts::lending(
            &mut ctx,
            |_| InOrder,
            vec![Box::new(enter), Box::new(enter)],
        )
    };
    let config = RunConfig::default().with_control(control.clone());
    // SAFETY: このファイルの頭の約束による。
    let outcome = unsafe {
        sequence::run(
            &scripts,
            Start {
                owners: &owners,
                host: &host(),
                config: &config,
            },
        )
    };
    assert_eq!(
        outcome.stop,
        fault(RunFault::Internal("a session run has one worker"))
    );
    assert!(outcome.retired.is_empty());
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert_eq!(control.worker_bits(0) & Control::END, 0);
}
