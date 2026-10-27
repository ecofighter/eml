//! `Async` のタスクとスコープの意味の核 (docs/spec/vm.md の「`Async` のタスク」)。VM の `Task`、`Scheduler`、フレームを
//! 知らず、走らせる仕事を `Runnable` と不透明な `WaitToken` で返す。ネイティブのコードが同じ核を `extern "C"` の入口から
//! 呼べるように、`std` と `eml_rt` だけに依存する。
//!
//! セルが持つ語は数えた参照で、VM が1回入れて1回出す。`Registry` はヒープに触れる `Drop` を持たないので、誤りで終わった
//! run と捨てた run では、`Scheduler` と一緒に触れずに捨てる。
//!
//! 不変条件: タスクのセルが `scope` を読むのは `start` と `finish` だけで、`Finished` の後は読まない。スコープの
//! `members` と `nested` の項は、いつも生きているセルを指す。スコープのセルを放すのは、閉じた後の `release_scope`
//! (`Take::Run` は根の待ちが `closed` で起きた後に呼ぶ) と、run の終わりの `sweep_all` だけである。
//! タスクのセルを放すのは、ハンドルを持つ側の `take`、自分のハンドルを落とした知らせの処理、`sweep_all` の3つだけで、
//! 待つ側はハンドルを持つので、待つ相手のセルはその `take` の前に放されない。

use std::sync::{Arc, Mutex, PoisonError};

use eml_rt::{Control, Ctx, Fault, HeapError, Mode, Word};

/// 再開するタスクに渡す `Std.Async.Wake` の `Woke` のタグ (docs/spec/stdlib.md の「`Std.Async`」)。
pub(crate) const WOKE: i64 = 0;

/// 登録表のセルの番号。添字を使い回すので、generation で古い番号を見分ける。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CellRef {
    index: u32,
    generation: u32,
}

impl CellRef {
    /// `Std.Async.Cell` の即値 `(generation << 32) | index` (docs/spec/stdlib.md の「`Std.Async`」)。
    fn encode(self) -> i64 {
        (i64::from(self.generation) << 32) | i64::from(self.index)
    }

    fn decode(c: i64) -> CellRef {
        CellRef {
            index: c as u32,
            generation: (c >> 32) as u32,
        }
    }
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
enum Waiter {
    /// 継続だけになって待つタスク。継続は待つタスクのセルが持つ。
    Task(CellRef),
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
    /// 継続だけになって待つか、実行待ちの列の後ろへ回った。再開する仕事が `Running` に戻す。
    Parked,
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
    /// スコープの属するタスクの一覧の中の位置。
    member_at: u32,
    waiters: Vec<Waiter>,
    /// 待っている間の継続。セルが参照を1つ持つ。
    k: Option<Word>,
    detached: bool,
}

struct ScopeCell {
    parent: Option<CellRef>,
    /// 親の入れ子の一覧の中の位置。
    nested_at: u32,
    /// まだ終わっていないタスク。
    members: Vec<CellRef>,
    nested: Vec<CellRef>,
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
    HandleDropped(CellRef),
}

/// ほかの worker やハンドルの破棄処理が核へ知らせる受け箱。
#[derive(Default)]
pub(crate) struct Inbox {
    notices: Mutex<Vec<Notice>>,
}

impl Inbox {
    fn push(&self, notice: Notice) {
        self.notices
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(notice);
    }

    fn take(&self) -> Vec<Notice> {
        std::mem::take(&mut *self.notices.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

/// `Task a` の `Foreign` の中身。
pub(crate) struct TaskHandle {
    cell: CellRef,
    inbox: Arc<Inbox>,
    /// `take` が受け取った。受け取ったハンドルはセルをもう指さないので、落ちても知らせない。
    taken: bool,
}

impl Drop for TaskHandle {
    // 破棄処理はヒープの `decref` の中で走るので、セルに触れずに知らせを積むだけにする。前の run のハンドルは、
    // 自分の死んだ受け箱に積むので、生きている登録表には届かない (docs/spec/vm.md の「`Async` のタスク」)
    fn drop(&mut self) {
        if !self.taken {
            self.inbox.push(Notice::HandleDropped(self.cell));
        }
    }
}

/// 登録表が数える仕事の回数。検査モードでなくても数えるが、`RunStats` に写すのは検査モードだけである
/// (docs/spec/vm.md の「`RunStats`」)。
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct TaskStats {
    /// `start` が作ったタスクのセルの数。
    pub(crate) tasks: u64,
    /// `park` と `ready` が渡された継続の数。
    pub(crate) parks: u64,
    /// スコープの最初の失敗より後に終わった、失敗のタスクの数。
    pub(crate) dropped_failures: u64,
    /// 取り消しが届いた回数。
    pub(crate) cancels: u64,
}

/// 1回の実行のセルの表。実行ごとに作る (docs/spec/vm.md の「`Async` のタスク」)。
pub(crate) struct Registry {
    slots: Vec<(u32, Option<CellKind>)>,
    free: Vec<u32>,
    pub(crate) inbox: Arc<Inbox>,
    pub(crate) stats: TaskStats,
}

impl Default for Registry {
    fn default() -> Registry {
        Registry {
            slots: Vec::new(),
            free: Vec::new(),
            inbox: Arc::new(Inbox::default()),
            stats: TaskStats::default(),
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
    pub(crate) fn open_root(&mut self, parent: Option<CellRef>) -> Result<CellRef, CellFault> {
        let nested_at = match parent {
            Some(p) => self.scope_mut(p)?.nested.len() as u32,
            None => 0,
        };
        let s = self.alloc(CellKind::Scope(ScopeCell {
            parent,
            nested_at,
            members: Vec::new(),
            nested: Vec::new(),
            first_failure: None,
            waiters: Vec::new(),
            closed: false,
        }));
        if let Some(p) = parent {
            self.scope_mut(p)?.nested.push(s);
        }
        Ok(s)
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
        let s = self.scope_mut(scope)?;
        if s.closed {
            return Err(CellFault::Internal("a task starts in a closed scope"));
        }
        let member_at = s.members.len() as u32;
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
            member_at,
            waiters: Vec::new(),
            k: None,
            detached: false,
        }));
        self.scope_mut(scope)?.members.push(cell);
        self.stats.tasks += 1;
        let handle = heap.alloc_foreign(
            TaskHandle {
                cell,
                inbox: Arc::clone(&self.inbox),
                taken: false,
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

    /// 再開する仕事を実行系が動かし始めた。
    pub(crate) fn resumed(&mut self, cell: CellRef) -> Result<(), CellFault> {
        let task = self.task_mut(cell)?;
        if !matches!(task.phase, Phase::Parked) {
            return Err(CellFault::Internal("a task resumes without waiting"));
        }
        task.phase = Phase::Running;
        Ok(())
    }

    /// 借りたハンドル `handle` のセルの番号を `Std.Async.Cell` の即値で返す。
    pub(crate) fn cell_of<M: Mode>(&self, ctx: &Ctx<M>, handle: Word) -> Result<i64, CellFault> {
        Ok(self.handle_cell(ctx, handle)?.encode())
    }

    /// タスク `me` が、継続 `k` でセル `c` が終わるか閉じるのを待つ。もう終わっていれば、すぐ再開する仕事を返す。
    /// どちらでも、`k` は継続の待ちか返す仕事に移る。終わったセルにも近道をせず、いつも継続を渡して再開する。
    pub(crate) fn park(
        &mut self,
        c: i64,
        me: CellRef,
        k: Word,
    ) -> Result<Option<Runnable>, CellFault> {
        let target = CellRef::decode(c);
        let done = match self.cell_mut(target)? {
            CellKind::Task(task) => matches!(task.phase, Phase::Finished),
            CellKind::Scope(scope) => scope.closed,
        };
        self.suspend(me)?;
        self.stats.parks += 1;
        if done {
            return Ok(Some(Runnable::Resume {
                cell: me,
                k,
                wake: WOKE,
            }));
        }
        self.task_mut(me)?.k = Some(k);
        let waiters = match self.cell_mut(target)? {
            CellKind::Task(task) => &mut task.waiters,
            CellKind::Scope(scope) => &mut scope.waiters,
        };
        waiters.push(Waiter::Task(me));
        Ok(None)
    }

    /// タスク `me` が、継続 `k` で実行待ちの列の後ろへ回る。
    pub(crate) fn ready(&mut self, me: CellRef, k: Word) -> Result<Runnable, CellFault> {
        self.suspend(me)?;
        self.stats.parks += 1;
        Ok(Runnable::Resume {
            cell: me,
            k,
            wake: WOKE,
        })
    }

    fn suspend(&mut self, me: CellRef) -> Result<(), CellFault> {
        let task = self.task_mut(me)?;
        if !matches!(task.phase, Phase::Running) {
            return Err(CellFault::Internal("a task waits without running"));
        }
        task.phase = Phase::Parked;
        Ok(())
    }

    /// ハンドルのセル。ハンドルの受け箱が登録表の受け箱と違えば、前の run のハンドルである。添字は run ごとに
    /// 使い回すので、generation では見分けられない。`Task a` は `Lin` で入力をまたいで残らないので、検査を通った
    /// プログラムからは届かない (docs/spec/vm.md の「`Async` のタスク」)。
    fn handle_cell<M: Mode>(&self, ctx: &Ctx<M>, handle: Word) -> Result<CellRef, CellFault> {
        // ハンドルは `Task a` の `Foreign` で、呼ぶ側が持つ唯一の参照である (`Task a` は `Lin`)
        let h = unsafe { ctx.heap().foreign_unique::<TaskHandle>(handle) }?;
        let h = unsafe { &*h };
        if !Arc::ptr_eq(&h.inbox, &self.inbox) {
            return Err(CellFault::Internal("a task handle outlived its run"));
        }
        Ok(h.cell)
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
        let (scope, at, detached) = (task.scope, task.member_at, task.detached);
        self.remove_member(scope, at)?;
        let task = self.task_mut(cell)?;
        task.phase = Phase::Finished;
        task.outcome = Some(outcome);
        for waiter in std::mem::take(&mut task.waiters) {
            self.wake(waiter, out)?;
        }
        if let Some(failure) = failure {
            let s = self.scope_mut(scope)?;
            if s.first_failure.is_none() {
                s.first_failure = Some(failure);
            } else {
                self.stats.dropped_failures += 1;
            }
        }
        let s = self.scope_mut(scope)?;
        if s.members.is_empty() {
            s.closed = true;
            for waiter in std::mem::take(&mut s.waiters) {
                self.wake(waiter, out)?;
            }
        }
        if detached {
            self.free_task(ctx, cell, depth_offset)?;
        }
        Ok(())
    }

    /// 起こす待ち手。継続で待つタスクは、セルの継続を取り出して再開する。
    fn wake(&mut self, waiter: Waiter, out: &mut Vec<Runnable>) -> Result<(), CellFault> {
        match waiter {
            Waiter::Task(t) => {
                if let Some(k) = self.task_mut(t)?.k.take() {
                    out.push(Runnable::Resume {
                        cell: t,
                        k,
                        wake: WOKE,
                    });
                }
            }
            Waiter::Token(token) => out.push(Runnable::Token(token)),
        }
        Ok(())
    }

    fn remove_member(&mut self, scope: CellRef, at: u32) -> Result<(), CellFault> {
        let s = self.scope_mut(scope)?;
        s.members.swap_remove(at as usize);
        if let Some(&moved) = s.members.get(at as usize) {
            self.task_mut(moved)?.member_at = at;
        }
        Ok(())
    }

    /// 閉じたスコープのセルを、親の一覧から外してから放す。
    fn release_scope(&mut self, s: CellRef) -> Result<(), CellFault> {
        let scope = self.scope_mut(s)?;
        let (parent, at) = (scope.parent, scope.nested_at);
        if let Some(p) = parent {
            let p = self.scope_mut(p)?;
            p.nested.swap_remove(at as usize);
            if let Some(&moved) = p.nested.get(at as usize) {
                self.scope_mut(moved)?.nested_at = at;
            }
        }
        self.release(s.index);
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
        let cell = self.handle_cell(ctx, handle)?;
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
            self.release_scope(scope)?;
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
        // ハンドルの参照は呼ぶ側から移ってきたもので、ここで手放す。受け取った印を先に付けるので、破棄処理は
        // 知らせを積まない
        unsafe {
            (*ctx.heap().foreign_unique::<TaskHandle>(handle)?).taken = true;
            ctx.heap_mut().decref(handle)?;
        }
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
                        task.detached = true;
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

/// タスクの底に着いた `exit n` を、終わりの欄に書く。`control.exit` を呼ぶのはここだけである。最初の書き手だけが
/// 真を返す。実行系は、`main` の底なら普通に戻り、ほかのタスクの底ならほかのタスクを動かさずに worker の入口へ戻る
/// (docs/spec/object-model.md の「実行の制御」)。
pub(crate) fn end_with_exit(control: &Control, n: i64) -> bool {
    control.exit(n)
}
