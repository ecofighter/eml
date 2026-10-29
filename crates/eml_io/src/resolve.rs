//! 本物の driver の名前の解決のスレッド (docs/spec/vm.md の「時計と driver」)。compio のプールの仕事は終わる前に
//! 鍵を手放せないので、名前の解決はプールに回さず、手放せる切り離したスレッドで行う。取り消し、期限、`quiesce`、
//! Ctrl-C は解決の結果を待たない。

use std::collections::{HashSet, VecDeque};
use std::io;
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::mpsc::{self, Receiver, Sender};
use std::task::Waker;
use std::thread;

use eml_rt::OpKey;

use crate::mask::spawn_masked;

/// 名前とポートから番地の一覧を引く関数。
pub(crate) type Lookup = fn(&str, u16) -> io::Result<Vec<SocketAddr>>;

/// 同時に走る、手放していない解決の数の上限。超えた解決は受け付けの順に待つ。
const RESOLVE_LIMIT: usize = 8;

/// 手放してまだ戻らない解決のスレッドの数の上限。これだけ詰まっている間は、新しい解決をその場で失敗させる。
/// 壊れた解決器はスレッドを増やし続けず、誤りとして見える。
const ABANDONED_LIMIT: usize = 64;

/// 解決のスレッドのスタックの大きさ。`getaddrinfo` のほかに大きなものを持たない。
const STACK: usize = 256 * 1024;

type Answer = (OpKey, io::Result<Vec<SocketAddr>>);

/// std の解決器。誤りも空の一覧も `NotFound` にする。
pub(crate) fn std_lookup(host: &str, port: u16) -> io::Result<Vec<SocketAddr>> {
    let addrs: Vec<SocketAddr> = (host, port)
        .to_socket_addrs()
        .map_err(|e| io::Error::new(io::ErrorKind::NotFound, e.to_string()))?
        .collect();
    if addrs.is_empty() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "no address"));
    }
    Ok(addrs)
}

pub(crate) struct Resolver {
    lookup: Lookup,
    /// スレッドは結果を送ってから waker を鳴らす。driver を落とすと受け手が落ち、走っているスレッドは送れずに終わる。
    sender: Sender<Answer>,
    receiver: Receiver<Answer>,
    waker: Waker,
    /// 走っていて、手放していない解決。
    live: HashSet<OpKey>,
    /// まだ始めていない解決。受け付けの順である。
    queue: VecDeque<(OpKey, String, u16)>,
    /// 手放して、まだ結果を送ってこない解決。
    abandoned: HashSet<OpKey>,
    /// 受け取ったが、driver がまだ接続の試行を始めていない結果。
    ready: Vec<Answer>,
}

impl Resolver {
    pub(crate) fn new(lookup: Lookup, waker: Waker) -> Resolver {
        let (sender, receiver) = mpsc::channel();
        Resolver {
            lookup,
            sender,
            receiver,
            waker,
            live: HashSet::new(),
            queue: VecDeque::new(),
            abandoned: HashSet::new(),
            ready: Vec::new(),
        }
    }

    /// `host` の解決を受け付ける。手放したスレッドが詰まっていれば、受け付けずに誤りを返す。
    pub(crate) fn submit(&mut self, key: OpKey, host: String, port: u16) -> io::Result<()> {
        // 戻ったスレッドを数えから外してから、上限と比べる
        self.receive();
        if self.abandoned.len() >= ABANDONED_LIMIT {
            return Err(io::Error::other("too many name lookups are stuck"));
        }
        self.queue.push_back((key, host, port));
        self.fill();
        Ok(())
    }

    /// 届いた結果を受け取る。手放した解決の結果は捨てる。
    pub(crate) fn receive(&mut self) {
        let mut freed = false;
        while let Ok((key, answer)) = self.receiver.try_recv() {
            if self.live.remove(&key) {
                self.ready.push((key, answer));
                freed = true;
            } else {
                self.abandoned.remove(&key);
            }
        }
        if freed {
            self.fill();
        }
    }

    /// 上限まで、待っている解決のスレッドを始める。始められなければ、その解決は誤りで終わる。
    fn fill(&mut self) {
        while self.live.len() < RESOLVE_LIMIT
            && let Some((key, host, port)) = self.queue.pop_front()
        {
            let (sender, waker, lookup) = (self.sender.clone(), self.waker.clone(), self.lookup);
            let builder = thread::Builder::new()
                .name("eml-resolve".to_string())
                .stack_size(STACK);
            // SIGINT と SIGTERM は VM のスレッドに届ける (docs/spec/vm.md の「上限と割り込み」)
            let spawned = spawn_masked(builder, move || {
                let answer = lookup(&host, port);
                // driver が落ちていれば送れないが、結果を待つ者もいない
                if sender.send((key, answer)).is_ok() {
                    waker.wake();
                }
            });
            match spawned {
                Ok(_) => {
                    self.live.insert(key);
                }
                Err(e) => self.ready.push((key, Err(e))),
            }
        }
    }

    /// 受け取った結果を渡す。
    pub(crate) fn answers(&mut self) -> Vec<Answer> {
        std::mem::take(&mut self.ready)
    }

    /// 渡していない結果があるか。
    pub(crate) fn answered(&self) -> bool {
        !self.ready.is_empty()
    }

    /// `key` の解決を手放す。解決だったら真を返し、呼ぶ側が取り消しの完了を作る。走っている解決のスレッドは手放して
    /// 数え、後から届く結果は捨てる。結果は鍵で見分けるので、スレッドが答えるまでその鍵の新しい解決を受け付けては
    /// ならない。受け付けると、古い答えが新しい解決に当たる。
    /// `IoDriver::submit` の約束 (完了を返した鍵はすぐ使い回せる) の例外で、呼ぶ側の VM は鍵を使い回さない。
    pub(crate) fn cancel(&mut self, key: OpKey) -> bool {
        if self.live.remove(&key) {
            self.abandoned.insert(key);
            self.fill();
            return true;
        }
        if let Some(i) = self.queue.iter().position(|(k, _, _)| *k == key) {
            self.queue.remove(i);
            return true;
        }
        if let Some(i) = self.ready.iter().position(|(k, _)| *k == key) {
            drop(self.ready.remove(i));
            return true;
        }
        false
    }

    /// `key` の解決が、待ちか実行中か受け取り済みか、手放してまだ答えていないか。
    pub(crate) fn holds(&self, key: OpKey) -> bool {
        self.live.contains(&key)
            || self.abandoned.contains(&key)
            || self.ready.iter().any(|(k, _)| *k == key)
            || self.queue.iter().any(|(k, _, _)| *k == key)
    }

    /// すべての解決を、完了を作らずに手放す。
    pub(crate) fn abandon_all(&mut self) {
        self.queue.clear();
        self.ready.clear();
        self.abandoned.extend(self.live.drain());
    }

    /// 待っているか走っている、手放していない解決と、渡していない結果の数。
    pub(crate) fn pending(&self) -> usize {
        self.live.len() + self.queue.len() + self.ready.len()
    }
}
