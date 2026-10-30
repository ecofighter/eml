//! ブロックする呼び出しのプール (docs/spec/vm.md の「時計と driver」)。スレッドは上限まで増え、満ちたら仕事はここの列で
//! 待つ。`submit` は列に積むだけで、呼んだスレッドを止めない。安全点を通らない待ちを作らないためである。

use std::collections::{HashSet, VecDeque};
use std::io;
use std::mem;
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle, ThreadId};
use std::time::Duration;

use super::backend::Backend;
use super::inbox::{Answer, Inbox, Ticket};

/// スレッドの数の上限。
pub(crate) const POOL_LIMIT: usize = 32;
/// 空いたスレッドが抜けるまでの長さ。
pub(crate) const POOL_IDLE: Duration = Duration::from_secs(10);

/// `Op::Job` の中身。
pub(crate) type Job = Box<dyn FnOnce() -> io::Result<usize> + Send + 'static>;

struct Shared {
    state: Mutex<State>,
    cv: Condvar,
    inbox: Arc<Inbox>,
}

#[derive(Default)]
struct State {
    queue: VecDeque<(Ticket, Job)>,
    /// 起こした数 (始まる前のものも含む) から、抜けた数を引いたもの。
    threads: usize,
    /// 列が空で `cv` を待つスレッドと、起こされてまだ lock を取り直していないスレッド。
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

pub(crate) struct Pool {
    shared: Arc<Shared>,
    handles: Vec<JoinHandle<()>>,
    limit: usize,
}

impl Pool {
    /// 核は `POOL_LIMIT` を渡す。単体テストは小さな上限で、上限の振る舞いを確かめる。
    pub(crate) fn new(inbox: Arc<Inbox>, limit: usize) -> Pool {
        Pool {
            shared: Arc::new(Shared {
                state: Mutex::default(),
                cv: Condvar::new(),
                inbox,
            }),
            handles: Vec::new(),
            limit,
        }
    }

    /// 仕事を列に積む。答えはいつも受け箱に届く。
    pub(crate) fn submit<B: Backend>(&mut self, backend: &B, ticket: Ticket, job: Job) {
        let spawn = {
            let mut state = self.shared.lock();
            state.queue.push_back((ticket, job));
            // 列を見に来るスレッドが仕事の数より少ないときだけ増やす。`idle == 0` で比べると、起こされてまだ
            // lock を取り直していないスレッドに2つ目の仕事が並び、起こしが失われる
            let spawn = state.queue.len() > state.idle && state.threads < self.limit;
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
                let at = state.queue.iter().position(|(t, _)| *t == ticket);
                let job = at.and_then(|at| state.queue.remove(at));
                drop(state);
                if let Some(job) = job {
                    drop(job);
                    self.shared.inbox.put(ticket, Answer::Job(Ok(Err(e))));
                }
            }
        }
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
        let Some((ticket, job)) = state.queue.pop_front() else {
            state.idle += 1;
            let (next, waited) = shared
                .cv
                .wait_timeout(state, POOL_IDLE)
                .unwrap_or_else(PoisonError::into_inner);
            state = next;
            state.idle -= 1;
            if waited.timed_out() && state.queue.is_empty() {
                break;
            }
            continue;
        };
        state.running.insert(me);
        drop(state);
        let answer = panic::catch_unwind(AssertUnwindSafe(job));
        // 答えを積む前に走っている印を外す。答えを見た driver がすぐ落ちても、このスレッドは join される側にいる。
        // 積む (受け箱の lock と起こし) のはプールの lock の外で行う
        shared.lock().running.remove(&me);
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
        let queued = mem::take(&mut state.queue);
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
