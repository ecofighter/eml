//! `Async` のタスクとスコープの意味の核 (docs/spec/vm.md の「`Async` のタスク」)。VM の `Task`、`Scheduler`、フレームを
//! 知らず、走らせる仕事を `Runnable` と不透明な `WaitToken` で返す。ネイティブのコードが同じ核を `extern "C"` の入口から
//! 呼べるように、`std` と `eml_rt` だけに依存する。
//!
//! セルが持つ語は数えた参照で、VM が1回入れて1回出す。`Registry` はヒープに触れる `Drop` を持たないので、誤りで終わった
//! run と捨てた run では、`Scheduler` と一緒に触れずに捨てる。
//!
//! 不変条件: タスクのセルが `scope` を読むのは `start` と `finish` だけで、`Finished` の後は読まない。スコープのセルを
//! 放すのは `live == 0` のとき (`Take::Run` は根の待ちが `closed` で起きた後、`sweep_all` は run の終わり) だけである。
//! タスクのセルを放すのは、ハンドルを持つ側の `take`、自分のハンドルを落とした知らせの処理、`sweep_all` の3つだけで、
//! 待つ側はハンドルを持つので、待つ相手のセルはその `take` の前に放されない。

use std::sync::{Arc, Mutex, PoisonError};

use eml_rt::{Ctx, Fault, HeapError, Mode, Word};

/// 再開するタスクに渡す `Std.Async.Wake` の `Woke` のタグ (docs/spec/stdlib.md の「`Std.Async`」)。
pub(crate) const WOKE: i64 = 0;

/// 登録表のセルの番号。添字を使い回すので、generation で古い番号を見分ける。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CellRef {
    index: u32,
    generation: u32,
}

/// token で待つものの番号。実行系が自分の単位 (VM では `TaskId`) に写す。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct WaitToken(pub u64);

/// 核が実行系に渡す仕事。
pub(crate) enum Runnable {
    /// タスク `cell` を始める。`closure` は入口 `start_task` に渡す閉包で、仕事が参照を1つ持つ。
    Start { cell: CellRef, closure: Word },
    /// 待っていたタスク `cell` を、継続 `k` に `wake` を渡して再開する。`k` は仕事が参照を1つ持つ。
    Resume { cell: CellRef, k: Word, wake: i64 },
    /// token で待つものを起こす。
    Token(WaitToken),
}

/// セルが終わるか閉じるのを待つもの。
pub(crate) enum Waiter {
    /// 継続だけになって待つタスク。`task` は待った側のセルで、`k` はセルが参照を1つ持つ。
    #[expect(dead_code, reason = "made by the park operation of the task waits")]
    Cont { task: CellRef, k: Word },
    /// スタックを持ったまま待つもの。`Async.run` を呼んだ VM の `Task` である。
    Token(WaitToken),
}

/// タスクの終わり方。
pub(crate) enum Outcome {
    /// `finish_ok` が預けた語そのもの。即値のこともある。ランタイムは写さず中身を見ない。
    Value(Word),
    /// IO の失敗。文言は Rust の文字列で持つので、失敗の語は持ち主をまたがない。
    Io {
        kind: i64,
        message: String,
    },
    Panic(String),
}

impl Outcome {
    /// 失敗なら、スコープに記録する写し。
    fn failure(&self) -> Option<Outcome> {
        match self {
            Outcome::Value(_) => None,
            Outcome::Io { kind, message } => Some(Outcome::Io {
                kind: *kind,
                message: message.clone(),
            }),
            Outcome::Panic(message) => Some(Outcome::Panic(message.clone())),
        }
    }
}

/// `take` が起こし直す失敗。
#[expect(dead_code, reason = "read when a task failure is raised again")]
pub(crate) enum Raise {
    Io { kind: i64, message: String },
    Panic(String),
}

/// `take` の種類。`Run` は根のスコープの最初の失敗を本体の値より先にし、根のスコープを放す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Take {
    Task,
    Run,
}

/// 核の操作の誤り。どれも run を止める。
#[derive(Debug)]
pub(crate) enum CellFault {
    Heap(HeapError),
    Internal(&'static str),
}

impl From<HeapError> for CellFault {
    fn from(error: HeapError) -> CellFault {
        CellFault::Heap(error)
    }
}

impl From<Fault> for CellFault {
    fn from(fault: Fault) -> CellFault {
        match fault {
            Fault::Heap(error) => CellFault::Heap(error),
            _ => CellFault::Internal("a heap operation of the task cells raised a non-heap fault"),
        }
    }
}

enum Phase {
    /// 始める仕事を実行系に渡したが、まだ動いていない。
    Placed,
    Running,
    Finished,
}

struct TaskCell {
    phase: Phase,
    outcome: Option<Outcome>,
    scope: CellRef,
    /// タスクの論理的な worker ID。速いモードでは 0 である。
    worker: u32,
    /// 始めたタスクの論理的な ID。始めるときの `transfer_back` に使う。
    spawner: u32,
    waiters: Vec<Waiter>,
    handle_dropped: bool,
}

struct ScopeCell {
    /// まだ終わっていないタスクの数。
    live: u32,
    /// 終わった順で最初の失敗。
    first_failure: Option<Outcome>,
    waiters: Vec<Waiter>,
    closed: bool,
}

enum CellKind {
    Task(TaskCell),
    Scope(ScopeCell),
}

/// ハンドルの `Drop` が積み、核が受け箱の処理で読む知らせ。
pub(crate) enum Notice {
    #[expect(dead_code, reason = "pushed by the Drop of a task handle")]
    HandleDropped(CellRef),
}

/// ほかの worker やハンドルの破棄処理が核へ知らせる受け箱。
#[derive(Default)]
pub(crate) struct Inbox {
    notices: Mutex<Vec<Notice>>,
}

impl Inbox {
    fn take(&self) -> Vec<Notice> {
        std::mem::take(&mut *self.notices.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

/// `Task a` の `Foreign` の中身。
pub(crate) struct TaskHandle {
    cell: CellRef,
    inbox: Arc<Inbox>,
}

/// 1回の実行のセルの表。実行ごとに作る (docs/spec/vm.md の「`Async` のタスク」)。
pub(crate) struct Registry {
    slots: Vec<(u32, Option<CellKind>)>,
    free: Vec<u32>,
    pub(crate) inbox: Arc<Inbox>,
}

impl Default for Registry {
    fn default() -> Registry {
        Registry {
            slots: Vec::new(),
            free: Vec::new(),
            inbox: Arc::new(Inbox::default()),
        }
    }
}

impl Registry {
    fn alloc(&mut self, kind: CellKind) -> CellRef {
        let index = match self.free.pop() {
            Some(index) => index,
            None => {
                self.slots.push((0, None));
                u32::try_from(self.slots.len() - 1).expect("the cells fit in u32 positions")
            }
        };
        let slot = &mut self.slots[index as usize];
        slot.1 = Some(kind);
        CellRef {
            index,
            generation: slot.0,
        }
    }

    /// 添字を使い回せるようにし、generation を1つ進めて古い番号を見分ける。
    fn release(&mut self, index: u32) {
        let slot = &mut self.slots[index as usize];
        slot.0 = slot.0.wrapping_add(1);
        slot.1 = None;
        self.free.push(index);
    }

    fn cell_mut(&mut self, cell: CellRef) -> Result<&mut CellKind, CellFault> {
        match self.slots.get_mut(cell.index as usize) {
            Some((generation, Some(kind))) if *generation == cell.generation => Ok(kind),
            _ => Err(CellFault::Internal("a cell reference names a freed cell")),
        }
    }

    fn task_mut(&mut self, cell: CellRef) -> Result<&mut TaskCell, CellFault> {
        match self.cell_mut(cell)? {
            CellKind::Task(task) => Ok(task),
            CellKind::Scope(_) => Err(CellFault::Internal("a task cell reference names a scope")),
        }
    }

    fn scope_mut(&mut self, cell: CellRef) -> Result<&mut ScopeCell, CellFault> {
        match self.cell_mut(cell)? {
            CellKind::Scope(scope) => Ok(scope),
            CellKind::Task(_) => Err(CellFault::Internal("a scope cell reference names a task")),
        }
    }

    /// タスクの論理的な worker ID。
    pub(crate) fn worker(&mut self, cell: CellRef) -> Result<u32, CellFault> {
        Ok(self.task_mut(cell)?.worker)
    }

    /// タスクが新しいタスクを加えるスコープ。
    pub(crate) fn scope_of(&mut self, cell: CellRef) -> Result<CellRef, CellFault> {
        Ok(self.task_mut(cell)?.scope)
    }

    /// スコープが閉じているか。
    pub(crate) fn closed(&mut self, scope: CellRef) -> Result<bool, CellFault> {
        Ok(self.scope_mut(scope)?.closed)
    }

    /// 根のスコープを開く。
    pub(crate) fn open_root(&mut self) -> CellRef {
        self.alloc(CellKind::Scope(ScopeCell {
            live: 0,
            first_failure: None,
            waiters: Vec::new(),
            closed: false,
        }))
    }

    /// スコープ `scope` に新しいタスクを置き、ハンドルと始める仕事を返す。`spawner` は呼んだタスクの論理的な ID で、
    /// `g` は入口に渡す閉包である。新しい ID に渡す前に、`g` とまだ印のないセッションの根 `roots` に共有の印を付ける
    /// (`par` の fork と同じ。docs/spec/vm.md の「`Async` のタスク」)。
    pub(crate) fn start<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        scope: CellRef,
        spawner: u32,
        g: Word,
        roots: &[Word],
    ) -> Result<(Word, Runnable), CellFault> {
        if self.scope_mut(scope)?.closed {
            return Err(CellFault::Internal("a task starts in a closed scope"));
        }
        // 印付けは送り手の ID のまま行い、`Ctx` の持つ語には触れない。値から届く evidence は退いたものだけである
        // (docs/spec/object-model.md の「evidence の3つの物体」)
        let heap = unsafe { ctx.heap_mut() };
        let worker = heap.fork_worker();
        for &root in roots {
            unsafe { heap.share(root, worker) }?;
        }
        unsafe { heap.share(g, worker) }?;
        let cell = self.alloc(CellKind::Task(TaskCell {
            phase: Phase::Placed,
            outcome: None,
            scope,
            worker,
            spawner,
            waiters: Vec::new(),
            handle_dropped: false,
        }));
        self.scope_mut(scope)?.live += 1;
        let handle = heap.alloc_foreign(
            TaskHandle {
                cell,
                inbox: Arc::clone(&self.inbox),
            },
            true,
        );
        Ok((handle, Runnable::Start { cell, closure: g }))
    }

    /// 始める仕事を実行系が動かし始めた。始めたタスクの ID を返す。実行系は、その ID から閉包を `transfer_back` する。
    pub(crate) fn started(&mut self, cell: CellRef) -> Result<u32, CellFault> {
        let task = self.task_mut(cell)?;
        if !matches!(task.phase, Phase::Placed) {
            return Err(CellFault::Internal("a task starts twice"));
        }
        task.phase = Phase::Running;
        Ok(task.spawner)
    }

    /// `cell` が終わるか閉じるまで、token で待つ。
    pub(crate) fn wait(&mut self, cell: CellRef, token: WaitToken) -> Result<(), CellFault> {
        let waiters = match self.cell_mut(cell)? {
            CellKind::Task(task) => &mut task.waiters,
            CellKind::Scope(scope) => &mut scope.waiters,
        };
        waiters.push(Waiter::Token(token));
        Ok(())
    }

    /// タスク `cell` を `outcome` で終える。起こす仕事を `out` に積む。失敗はスコープの最初の失敗として記録する。
    /// スコープの最後のタスクならスコープを閉じ、その待ち手も起こす。ハンドルがもう落ちていれば、セルをすぐ放す。
    /// `depth_offset` は呼んだ実行系のタスクの深さの起点で、セルを放すときに `Ctx` へ戻す。
    pub(crate) fn finish<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        cell: CellRef,
        outcome: Outcome,
        depth_offset: usize,
        out: &mut Vec<Runnable>,
    ) -> Result<(), CellFault> {
        let failure = outcome.failure();
        let task = self.task_mut(cell)?;
        if !matches!(task.phase, Phase::Running) {
            return Err(CellFault::Internal("a task finishes without running"));
        }
        task.phase = Phase::Finished;
        task.outcome = Some(outcome);
        let scope = task.scope;
        let dropped = task.handle_dropped;
        out.extend(std::mem::take(&mut task.waiters).into_iter().map(wake));
        let scope = self.scope_mut(scope)?;
        if let Some(failure) = failure
            && scope.first_failure.is_none()
        {
            scope.first_failure = Some(failure);
        }
        scope.live = scope.live.checked_sub(1).ok_or(CellFault::Internal(
            "a scope ends more tasks than it started",
        ))?;
        if scope.live == 0 {
            scope.closed = true;
            out.extend(std::mem::take(&mut scope.waiters).into_iter().map(wake));
        }
        if dropped {
            self.free_task(ctx, cell, depth_offset)?;
        }
        Ok(())
    }

    /// ハンドル `handle` を消費して、終わったタスクの値を返すか、失敗を返す。`handle` は呼ぶ側が持つ参照である。
    /// 値は終わったタスクの ID から今の worker へ戻し、セルを放す。
    pub(crate) fn take<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        handle: Word,
        kind: Take,
        depth_offset: usize,
    ) -> Result<Result<Word, Raise>, CellFault> {
        // ハンドルは `Task a` の `Foreign` で、呼ぶ側が持つ唯一の参照である (`Task a` は `Lin`)
        let cell = {
            let h = unsafe { ctx.heap().foreign_unique::<TaskHandle>(handle) }?;
            let h = unsafe { &*h };
            if !Arc::ptr_eq(&h.inbox, &self.inbox) {
                return Err(CellFault::Internal("a task handle outlived its run"));
            }
            h.cell
        };
        let task = self.task_mut(cell)?;
        if !matches!(task.phase, Phase::Finished) {
            return Err(CellFault::Internal("awaiting an unfinished task"));
        }
        let (worker, scope) = (task.worker, task.scope);
        let outcome = task
            .outcome
            .take()
            .ok_or(CellFault::Internal("a finished task has no outcome"))?;
        let mut raised = None;
        if kind == Take::Run {
            let root = self.scope_mut(scope)?;
            if !root.closed {
                return Err(CellFault::Internal("a run ends before its scope closes"));
            }
            raised = root.first_failure.take();
            self.release(scope.index);
        }
        let result = match (raised, outcome) {
            (Some(failure), outcome) => {
                if let Outcome::Value(w) = outcome {
                    drop_value(ctx, worker, w, depth_offset)?;
                }
                Err(raise(failure))
            }
            (None, Outcome::Value(w)) => {
                // 値はタスクが `finish_ok` に預けた参照で、受け取る側だけが持つ
                unsafe { ctx.heap_mut().transfer_back(w, worker) }?;
                Ok(w)
            }
            (None, failure) => Err(raise(failure)),
        };
        // ハンドルの参照は呼ぶ側から移ってきたもので、ここで手放す
        unsafe { ctx.heap_mut().decref(handle) }?;
        self.free_task(ctx, cell, depth_offset)?;
        Ok(result)
    }

    /// 受け箱の知らせを処理する。lock の下では取り出すだけにする。
    pub(crate) fn drain<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        depth_offset: usize,
    ) -> Result<(), CellFault> {
        for notice in self.inbox.take() {
            match notice {
                Notice::HandleDropped(cell) => {
                    let task = self.task_mut(cell)?;
                    if matches!(task.phase, Phase::Finished) {
                        self.free_task(ctx, cell, depth_offset)?;
                    } else {
                        task.handle_dropped = true;
                    }
                }
            }
        }
        Ok(())
    }

    /// run の終わりに、残ったセルをすべて放す。その間に積まれた知らせは受け箱ごと捨てる。
    pub(crate) fn sweep_all<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        depth_offset: usize,
    ) -> Result<(), CellFault> {
        for index in 0..self.slots.len() {
            let (generation, kind) = &self.slots[index];
            let cell = CellRef {
                index: index as u32,
                generation: *generation,
            };
            match kind {
                Some(CellKind::Task(_)) => self.free_task(ctx, cell, depth_offset)?,
                Some(CellKind::Scope(_)) => self.release(cell.index),
                None => {}
            }
        }
        drop(self.inbox.take());
        Ok(())
    }

    /// タスクのセルを放す。受け取られなかった値をタスクの ID の下で落とし、ID を退かせる。退かせるときに ID の物体が
    /// 残っていれば、`par` の join と同じく `TransferLeak` がヒープの誤りとして出る。
    fn free_task<M: Mode>(
        &mut self,
        ctx: &mut Ctx<M>,
        cell: CellRef,
        depth_offset: usize,
    ) -> Result<(), CellFault> {
        let task = self.task_mut(cell)?;
        let (worker, outcome) = (task.worker, task.outcome.take());
        if let Some(Outcome::Value(w)) = outcome {
            drop_value(ctx, worker, w, depth_offset)?;
        }
        unsafe { ctx.heap_mut() }.retire_worker(worker)?;
        self.release(cell.index);
        Ok(())
    }
}

/// 待ち手を起こす仕事にする。継続で待つタスクは、待った側のセルの worker で再開する。
fn wake(waiter: Waiter) -> Runnable {
    match waiter {
        Waiter::Cont { task, k } => Runnable::Resume {
            cell: task,
            k,
            wake: WOKE,
        },
        Waiter::Token(token) => Runnable::Token(token),
    }
}

fn raise(failure: Outcome) -> Raise {
    match failure {
        Outcome::Io { kind, message } => Raise::Io { kind, message },
        Outcome::Panic(message) => Raise::Panic(message),
        Outcome::Value(_) => unreachable!("a failure is not a value"),
    }
}

/// 受け取られない値を、値を持つタスクの ID の下で落とす。`decref` は即値も、破棄処理を持つ `Foreign` も受ける。
fn drop_value<M: Mode>(
    ctx: &mut Ctx<M>,
    worker: u32,
    w: Word,
    depth_offset: usize,
) -> Result<(), CellFault> {
    with_worker(ctx, worker, depth_offset, |ctx| unsafe {
        ctx.heap_mut().decref(w)
    })?;
    Ok(())
}

/// ほかのタスクの ID の物体を放すときの切り替え。`evv` を空にして ID `id` に替え、`f` の後で元の `evv`、worker、
/// 深さの起点 `depth_offset` に戻す。RC には触れない (docs/spec/object-model.md の「タスクの切り替え」)。
pub(crate) fn with_worker<M: Mode, T>(
    ctx: &mut Ctx<M>,
    id: u32,
    depth_offset: usize,
    f: impl FnOnce(&mut Ctx<M>) -> T,
) -> T {
    let prev = ctx.heap().worker();
    // 入れる `()` は即値で、取り出した `evv` は戻すときにそのまま返すので、参照の数は変わらない
    let evv = unsafe { ctx.switch_task(Word::UNIT, id, 0) };
    let result = f(ctx);
    let _unit = unsafe { ctx.switch_task(evv, prev, depth_offset) };
    result
}
