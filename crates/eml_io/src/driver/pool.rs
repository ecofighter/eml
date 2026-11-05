//! ブロックする呼び出しのプール (docs/spec/vm.md の「時計と driver」)。スレッドは上限まで増え、満ちたら仕事はここの列で
//! 待つ。`submit` は列に積むだけで、呼んだスレッドを止めない。安全点を通らない待ちを作らないためである。`Op::Job` の
//! 仕事が同時に使うスレッドには別の上限があり、ファイルの仕事はどのスレッドも使える。

use std::collections::{HashSet, VecDeque};
use std::mem;
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle, ThreadId};
use std::time::Duration;

use super::backend::{Backend, Job};
use super::inbox::{Answer, Inbox, Ticket};

/// スレッドの数の上限。
pub(crate) const POOL_LIMIT: usize = 32;
/// 同時に走る `Op::Job` の上限。残りのスレッドは、戻らない C の呼び出しの下でもファイルの仕事に残す。
pub(crate) const JOB_LIMIT: usize = 24;
/// 空いたスレッドが抜けるまでの長さ。
pub(crate) const POOL_IDLE: Duration = Duration::from_secs(10);

struct Shared {
    state: Mutex<State>,
    cv: Condvar,
    inbox: Arc<Inbox>,
    job_limit: usize,
}

#[derive(Default)]
struct State {
    files: VecDeque<(Ticket, Job)>,
    values: VecDeque<(Ticket, Job)>,
    /// 取ってから、`running` を外すまでの `Op::Job` の仕事の数。
    running_values: usize,
    /// 起こした数 (始まる前のものも含む) から、抜けた数を引いたもの。
    threads: usize,
    /// 始められる仕事がなく `cv` を待つスレッドと、起こされてまだ lock を取り直していないスレッド。
    idle: usize,
    /// 仕事を取ってから、答えを積む直前までのスレッド。`Drop` はこれを切り離し、ほかを join する。
    running: HashSet<ThreadId>,
    closed: bool,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl State {
    /// 今スレッドが空けば取れる仕事の数。
    fn startable(&self, job_limit: usize) -> usize {
        let room = job_limit.saturating_sub(self.running_values);
        self.files.len() + self.values.len().min(room)
    }

    /// 始められる仕事のうち、いちばん古いファイルの仕事を、なければいちばん古い `Op::Job` の仕事を取る。ファイルの
    /// 仕事を先にするので、多くのファイルの仕事が C の呼び出しを遅らせることはあっても、逆はない。
    fn take(&mut self, job_limit: usize) -> Option<(Ticket, Job)> {
        if let Some(job) = self.files.pop_front() {
            return Some(job);
        }
        if self.running_values >= job_limit {
            return None;
        }
        let job = self.values.pop_front()?;
        self.running_values += 1;
        Some(job)
    }

    /// 札の仕事が列にあれば外す。なければ、どれかのスレッドが取っていて、そちらが答える。
    fn remove(&mut self, ticket: Ticket) -> Option<Job> {
        let queue = if self.files.iter().any(|(t, _)| *t == ticket) {
            &mut self.files
        } else {
            &mut self.values
        };
        let at = queue.iter().position(|(t, _)| *t == ticket)?;
        queue.remove(at).map(|(_, job)| job)
    }
}

pub(crate) struct Pool {
    shared: Arc<Shared>,
    handles: Vec<JoinHandle<()>>,
    limit: usize,
}

impl Pool {
    /// 核は `POOL_LIMIT` と `JOB_LIMIT` を渡す。単体テストは小さな上限で、上限の振る舞いを確かめる。
    pub(crate) fn new(inbox: Arc<Inbox>, limit: usize, job_limit: usize) -> Pool {
        Pool {
            shared: Arc::new(Shared {
                state: Mutex::default(),
                cv: Condvar::new(),
                inbox,
                job_limit,
            }),
            handles: Vec::new(),
            limit,
        }
    }

    /// 仕事を列に積む。答えはいつも受け箱に届く。
    pub(crate) fn submit<B: Backend>(&mut self, backend: &B, ticket: Ticket, job: Job) {
        let spawn = {
            let mut state = self.shared.lock();
            match job {
                Job::File(_) => state.files.push_back((ticket, job)),
                Job::Value(_) => state.values.push_back((ticket, job)),
            }
            // 列を見に来るスレッドが始められる仕事の数より少ないときだけ増やす。`idle == 0` で比べると、起こされて
            // まだ lock を取り直していないスレッドに2つ目の仕事が並び、起こしが失われる。上限で待つ `Op::Job` の
            // 仕事は数えない。走っている `Op::Job` を終えたスレッドが自分で取るからである
            let spawn =
                state.startable(self.shared.job_limit) > state.idle && state.threads < self.limit;
            if spawn {
                state.threads += 1;
            }
            spawn
        };
        self.shared.cv.notify_one();
        if !spawn {
            return;
        }
        // 空き時間で抜けたスレッドを片付ける。終わったスレッドの join は待たない
        let mut i = 0;
        while i < self.handles.len() {
            if self.handles[i].is_finished() {
                let _ = self.handles.swap_remove(i).join();
            } else {
                i += 1;
            }
        }
        let shared = Arc::clone(&self.shared);
        let builder = thread::Builder::new().name("eml-pool".to_string());
        match backend.spawn(builder, Box::new(move || work(&shared))) {
            Ok(handle) => self.handles.push(handle),
            Err(e) => {
                let mut state = self.shared.lock();
                state.threads -= 1;
                // 列に残っていれば、この仕事は誰も取っていない。ほかのスレッドが取っていれば、そちらが答える。
                // どちらでも完了は1回になる
                let job = state.remove(ticket);
                drop(state);
                if let Some(job) = job {
                    drop(job);
                    self.shared.inbox.put(ticket, Answer::Job(Ok(Err(e))));
                }
            }
        }
    }

    /// 列で待つ仕事を、走らせずに外して返す。取られた仕事は外せず、`None` を返す。返した仕事は、呼ぶ側がプールの
    /// lock の外で落とす。
    pub(crate) fn forget(&mut self, ticket: Ticket) -> Option<Job> {
        self.shared.lock().remove(ticket)
    }
}

/// プールのスレッドの本体。
fn work(shared: &Shared) {
    let me = thread::current().id();
    let mut state = shared.lock();
    loop {
        if state.closed {
            break;
        }
        let Some((ticket, job)) = state.take(shared.job_limit) else {
            state.idle += 1;
            let (next, waited) = shared
                .cv
                .wait_timeout(state, POOL_IDLE)
                .unwrap_or_else(PoisonError::into_inner);
            state = next;
            state.idle -= 1;
            if waited.timed_out() && state.startable(shared.job_limit) == 0 {
                break;
            }
            continue;
        };
        let value = matches!(job, Job::Value(_));
        state.running.insert(me);
        drop(state);
        let answer = panic::catch_unwind(AssertUnwindSafe(|| job.run()));
        // 答えを積む前に走っている印を外す。答えを見た driver がすぐ落ちても、このスレッドは join される側にいる。
        // 積む (受け箱の lock と起こし) のはプールの lock の外で行う。`Op::Job` の数も同じ区間で減らし、上限で
        // 待つ仕事はこのスレッドが loop の頭で取る。区間の外で減らすと、その間の `submit` が古い数で起こしを決める
        state = shared.lock();
        state.running.remove(&me);
        if value {
            state.running_values -= 1;
        }
        drop(state);
        shared.inbox.push(ticket, Answer::Job(answer));
        state = shared.lock();
    }
    state.threads -= 1;
}

impl Drop for Pool {
    /// 始まっていない仕事は走らせずに捨てる。仕事を走らせているスレッドは切り離し、ほかは join する。切り離した
    /// スレッドは受け箱を自分で持つので、driver が消えた後も答えを積んで終わる。
    fn drop(&mut self) {
        let mut state = self.shared.lock();
        state.closed = true;
        let queued = (mem::take(&mut state.files), mem::take(&mut state.values));
        let (running, idle): (Vec<_>, Vec<_>) = mem::take(&mut self.handles)
            .into_iter()
            .partition(|h| state.running.contains(&h.thread().id()));
        drop(state);
        self.shared.cv.notify_all();
        // 利用者の値の `Drop` は lock の外で走らせる
        drop(queued);
        drop(running);
        for handle in idle {
            let _ = handle.join();
        }
    }
}
