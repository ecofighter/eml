//! 名前の解決のスレッド (docs/spec/vm.md の「時計と driver」)。プールの仕事は終わるまで待ちきるが、解決は取り消し、
//! 期限、`quiesce` で手放せるようにするため、プールに回さず自分のスレッドで行う。答えは札で当てるので、手放した
//! 解決の遅れた答えが、同じ鍵を使い回した操作に当たらない。

use std::collections::{HashMap, VecDeque};
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use super::backend::Backend;
use super::inbox::{Answer, Inbox, Ticket};

/// 名前とポートから番地の一覧を引く関数。
pub(crate) type Lookup = fn(&str, u16) -> io::Result<Vec<SocketAddr>>;

/// 同時に走る、手放していない解決の数の上限。超えた解決は受け付けの順に待つ。
pub(crate) const RESOLVE_LIMIT: usize = 8;

/// 手放してまだ答えない解決のスレッドの数の上限。これだけ詰まっている間は、新しい解決をその場で失敗させる。
/// 壊れた解決器はスレッドを増やし続けず、誤りとして見える。
pub(crate) const ABANDONED_LIMIT: usize = 64;

/// 解決のスレッドのスタックの大きさ。`getaddrinfo` のほかに大きなものを持たない。
const STACK: usize = 256 * 1024;

pub(crate) struct Resolver {
    lookup: Lookup,
    inbox: Arc<Inbox>,
    /// 走っていて、手放していない解決。
    live: HashMap<Ticket, JoinHandle<()>>,
    /// まだ始めていない解決。受け付けの順である。
    queue: VecDeque<(Ticket, String, u16)>,
    /// 手放して、まだ答えを引き取っていない解決。
    abandoned: HashMap<Ticket, JoinHandle<()>>,
}

impl Resolver {
    pub(crate) fn new(lookup: Lookup, inbox: Arc<Inbox>) -> Resolver {
        Resolver {
            lookup,
            inbox,
            live: HashMap::new(),
            queue: VecDeque::new(),
            abandoned: HashMap::new(),
        }
    }

    /// `host` の解決を受け付ける。手放したスレッドが詰まっていれば、受け付けずに誤りを返す。
    pub(crate) fn submit<B: Backend>(
        &mut self,
        backend: &B,
        ticket: Ticket,
        host: String,
        port: u16,
    ) -> io::Result<()> {
        // 答えて終わったスレッドを、受け箱の引き取りを待たずに数えから外してから上限と比べる
        for (_, handle) in self.abandoned.extract_if(|_, h| h.is_finished()) {
            let _ = handle.join();
        }
        if self.abandoned.len() >= ABANDONED_LIMIT {
            return Err(io::Error::other("too many name lookups are stuck"));
        }
        self.queue.push_back((ticket, host, port));
        self.fill(backend);
        Ok(())
    }

    /// 上限まで、待っている解決のスレッドを始める。始められなければ、その解決は誤りで終わる。
    fn fill<B: Backend>(&mut self, backend: &B) {
        while self.live.len() < RESOLVE_LIMIT
            && let Some((ticket, host, port)) = self.queue.pop_front()
        {
            let (inbox, lookup) = (Arc::clone(&self.inbox), self.lookup);
            let builder = thread::Builder::new()
                .name("eml-resolve".to_string())
                .stack_size(STACK);
            let body = move || inbox.push(ticket, Answer::Lookup(lookup(&host, port)));
            match backend.spawn(builder, Box::new(body)) {
                Ok(handle) => {
                    self.live.insert(ticket, handle);
                }
                Err(e) => self.inbox.put(ticket, Answer::Lookup(Err(e))),
            }
        }
    }

    /// `ticket` の答えを driver が引き取った。答えたスレッドはすぐ終わるので join する。
    pub(crate) fn answered<B: Backend>(&mut self, backend: &B, ticket: Ticket) {
        if let Some(handle) = self.live.remove(&ticket) {
            let _ = handle.join();
            self.fill(backend);
        } else if let Some(handle) = self.abandoned.remove(&ticket) {
            let _ = handle.join();
        }
    }

    /// `ticket` の解決を手放す。走っているスレッドは手放した数に移し、待っている解決は列から外す。
    pub(crate) fn cancel<B: Backend>(&mut self, backend: &B, ticket: Ticket) {
        if let Some(handle) = self.live.remove(&ticket) {
            self.abandoned.insert(ticket, handle);
            self.fill(backend);
        } else if let Some(at) = self.queue.iter().position(|(t, _, _)| *t == ticket) {
            self.queue.remove(at);
        }
    }

    /// すべての解決を手放す。
    pub(crate) fn abandon_all(&mut self) {
        self.queue.clear();
        self.abandoned.extend(self.live.drain());
    }
}
