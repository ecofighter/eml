//! ほかのスレッドから読むフックの値と、方針に知らせる地点 (docs/spec/runtime.mdの「フック」)。

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread;

use eml_sched::job::{Item, Root};
use eml_sched::policy::{Point, Policy};
use eml_sched::run::{NO_CELL, Phase, Signs};
use eml_sched::script::Scripts;
use eml_steps::Label;
use eml_steps::plan::Step;
use eml_steps::verdict::Effect as Verdict;

use crate::support::{control, finish, open_root, run, spawn, started};

/// ほかのスレッドで、渡された`Signs`のworker 0の値を読んで返す読み手。
struct Reader {
    ask: Mutex<Sender<Arc<Signs>>>,
    answer: Mutex<Receiver<(u64, Phase)>>,
}

impl Reader {
    fn start() -> (Reader, thread::JoinHandle<()>) {
        let (ask, asked) = channel::<Arc<Signs>>();
        let (answered, answer) = channel();
        let reader = thread::spawn(move || {
            for signs in asked {
                answered.send((signs.current(0), signs.phase(0))).unwrap();
            }
        });
        let reader_end = Reader {
            ask: Mutex::new(ask),
            answer: Mutex::new(answer),
        };
        (reader_end, reader)
    }

    /// 読み手に読ませ、その答えを待つ。
    fn read(&self, signs: &Arc<Signs>) -> (u64, Phase) {
        self.ask.lock().unwrap().send(signs.clone()).unwrap();
        self.answer.lock().unwrap().recv().unwrap()
    }
}

#[test]
fn current_names_the_running_item_from_another_thread() {
    let (reader, thread) = Reader::start();
    let seen = Mutex::new(Vec::new());
    let scripts = crate::support::one(|core, ctx, item| {
        let read = reader.read(core.signs());
        match item {
            Item::Root(Root::Main) => {
                seen.lock().unwrap().push((read, NO_CELL));
                let scope = open_root(core);
                spawn(core, ctx, scope);
            }
            Item::Entry(entry) => {
                seen.lock()
                    .unwrap()
                    .push((read, entry.cell.encode() as u64));
                started(core, entry.cell);
                finish(core, ctx, entry.cell);
            }
            Item::Root(Root::Result) => seen.lock().unwrap().push((read, NO_CELL)),
        }
        Ok(())
    });
    let outcome = run(&scripts, &control(1));
    assert_eq!(outcome.stop, None);
    drop(scripts);
    drop(reader);
    thread.join().unwrap();
    let seen = seen.into_inner().unwrap();
    assert_eq!(seen.len(), 3);
    for ((current, phase), expected) in seen {
        assert_eq!(current, expected);
        assert_eq!(phase, Phase::Run);
    }
}

/// 判定の最初の効果で読み手に読ませる方針。worker 0の判定は眠りである。
struct AskAtCount<'r> {
    reader: &'r Reader,
    signs: Arc<Mutex<Option<Arc<Signs>>>>,
    seen: Arc<Mutex<Vec<(u64, Phase)>>>,
}

impl Policy for AskAtCount<'_> {
    fn at(&self, _: u32, point: Point) {
        if point == Point::Kernel(Label::Verdict(Verdict::Count)) {
            let signs = self.signs.lock().unwrap().clone().expect("main ran");
            self.seen.lock().unwrap().push(self.reader.read(&signs));
        }
    }
}

#[test]
fn phase_shows_run_and_idle_from_another_thread() {
    let (reader, thread) = Reader::start();
    let signs = Arc::new(Mutex::new(None));
    let at_count = Arc::new(Mutex::new(Vec::new()));
    let in_items = Mutex::new(Vec::new());
    let scripts = Scripts::new(
        {
            let (reader, signs, seen) = (&reader, signs.clone(), at_count.clone());
            move |_| AskAtCount {
                reader,
                signs: signs.clone(),
                seen: seen.clone(),
            }
        },
        vec![Box::new(
            |core: &mut eml_sched::core::WorkerCore<AskAtCount<'_>>,
             ctx: &mut eml_rt::Ctx<eml_rt::Checked>,
             item: Item| {
                *signs.lock().unwrap() = Some(core.signs().clone());
                in_items.lock().unwrap().push(reader.read(core.signs()).1);
                match item {
                    Item::Root(Root::Main) => {
                        let scope = open_root(core);
                        spawn(core, ctx, scope);
                    }
                    Item::Entry(entry) => {
                        started(core, entry.cell);
                        finish(core, ctx, entry.cell);
                    }
                    Item::Root(Root::Result) => {}
                }
                Ok(())
            },
        )],
    );
    let outcome = run(&scripts, &control(1));
    assert_eq!(outcome.stop, None);
    drop(scripts);
    drop(reader);
    thread.join().unwrap();
    assert_eq!(*in_items.lock().unwrap(), [Phase::Run; 3]);
    assert_eq!(*at_count.lock().unwrap(), [(NO_CELL, Phase::Idle)]);
}

/// 地点を順に記録する方針。
#[derive(Clone)]
struct Record(Arc<Mutex<Vec<Label>>>);

impl Policy for Record {
    fn at(&self, _: u32, point: Point) {
        if let Point::Kernel(label) = point {
            self.0.lock().unwrap().push(label);
        }
    }
}

/// `main`がタスクを1つ置くrunで、インタプリタが行う手順と効果は、どれも行う前に`at`を通る。地点の列は、そのrunの手順
/// と判定の効果の列そのものである。
#[test]
fn every_effect_an_interpreter_performs_passes_through_at() {
    let labels = Record(Arc::new(Mutex::new(Vec::new())));
    let scripts = Scripts::new(
        {
            let labels = labels.clone();
            move |_| labels.clone()
        },
        vec![Box::new(
            |core: &mut eml_sched::core::WorkerCore<Record>,
             ctx: &mut eml_rt::Ctx<eml_rt::Checked>,
             item: Item| {
                match item {
                    Item::Root(Root::Main) => {
                        let scope = open_root(core);
                        spawn(core, ctx, scope);
                    }
                    Item::Entry(entry) => {
                        started(core, entry.cell);
                        finish(core, ctx, entry.cell);
                    }
                    Item::Root(Root::Result) => {}
                }
                Ok(())
            },
        )],
    );
    let outcome = run(&scripts, &control(1));
    assert_eq!(outcome.stop, None);
    use eml_steps::plan::Root as R;
    let expected = [
        Label::Loop(Step::Root(R::Main)),
        // 準備のできた列にタスクがあるので、空でないときの源を見てから、通りを始める
        Label::Loop(Step::Detached),
        Label::Loop(Step::Look),
        Label::Loop(Step::Inbox),
        Label::Loop(Step::Ready),
        Label::Loop(Step::Detached),
        Label::Loop(Step::Foreign),
        Label::Loop(Step::Look),
        Label::Loop(Step::Idle),
        Label::Verdict(Verdict::Count),
        Label::Verdict(Verdict::Lists),
        Label::Verdict(Verdict::Registry),
        Label::Verdict(Verdict::Own),
        Label::Verdict(Verdict::RootClosed),
        Label::Loop(Step::Root(R::Result)),
        Label::Loop(Step::Return),
    ];
    assert_eq!(*labels.0.lock().unwrap(), expected);
}
