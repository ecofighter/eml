//! プールと解決器のスレッドが答えを置く受け箱。答えは札で driver の操作に当てる (docs/spec/vm.md の「時計と driver」)。

use std::collections::VecDeque;
use std::io;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use eml_rt::IoWaker;

/// driver の中で1度しか使わない番号。鍵は呼ぶ側が使い回すので、スレッドの答えは鍵でなくこれで当てる。
pub(crate) type Ticket = u64;

pub(crate) enum Answer {
    /// `Err` は仕事の panic の payload
    Job(thread::Result<io::Result<usize>>),
    Lookup(io::Result<Vec<SocketAddr>>),
}

pub(crate) struct Inbox {
    arrived: Mutex<Vec<(Ticket, Answer)>>,
    waker: Arc<dyn IoWaker>,
}

impl Inbox {
    pub(crate) fn new(waker: Arc<dyn IoWaker>) -> Arc<Inbox> {
        Arc::new(Inbox {
            arrived: Mutex::new(Vec::new()),
            waker,
        })
    }

    /// ほかのスレッドから積む。積んでから鳴らすので、起きた driver は必ずこの答えを見る。
    pub(crate) fn push(&self, ticket: Ticket, answer: Answer) {
        self.put(ticket, answer);
        self.waker.wake();
    }

    /// driver のスレッドから積む (スレッドを作れなかったとき)。次の引き取りが拾うので、鳴らさない。
    pub(crate) fn put(&self, ticket: Ticket, answer: Answer) {
        self.arrived
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((ticket, answer));
    }

    /// 着いた答えを `out` の後ろへ移す。どちらの列も容量を残すので、引き取りのたびに確保しない。
    pub(crate) fn take(&self, out: &mut VecDeque<(Ticket, Answer)>) {
        out.extend(
            self.arrived
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .drain(..),
        );
    }
}
