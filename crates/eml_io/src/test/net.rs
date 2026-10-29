//! テストの driver の仮想のネットワーク (docs/spec/vm.md の「時計と driver」)。仮想のホストは1つで、ポートの
//! 名前空間も1つである。リスナーか接続した端が持つ間、そのポートは使っている。

use std::collections::{HashMap, HashSet, VecDeque};
use std::io;
use std::net::IpAddr;

use eml_rt::{Done, IoHandle};

/// 待ち受けのポートと接続する側のポートに振る範囲。
const FIRST_PORT: u16 = 49152;
const LAST_PORT: u16 = 65535;

/// リスナーの backlog。満ちた backlog への接続は、accept が席を空けるまで待つ。
const BACKLOG: usize = 1024;

/// 仮想のホストの名前を確かめる。IP アドレスの綴りはどれも仮想のホストを指し、`localhost` は `127.0.0.1` に
/// 解決する。ほかの名前は `NotFound` である。`Network::virtual_connect` と同じく、テストの driver の外の
/// `eml_io` のモジュールからも仮想の相手を作れるように、決着の処理に埋め込まずに1つの関数にしておく。
pub(crate) fn check_virtual_host(host: &str) -> io::Result<()> {
    if host == "localhost" || host.parse::<IpAddr>().is_ok() {
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::NotFound, "unknown host"))
    }
}

/// `Network::virtual_connect` の結果。
pub(crate) enum Connection {
    /// ポートで待ち受けるリスナーがない。
    Refused,
    /// リスナーの backlog が満ちている。
    Pending,
    /// 端の対を作り、相手の端をリスナーの backlog に入れた。
    Connected { client_end: IoHandle },
}

/// つながった2つの端の片方。
pub(super) struct End {
    pub(super) peer: IoHandle,
    /// 相手が書き、この端がまだ読んでいないバイト。
    pub(super) inbox: VecDeque<u8>,
    /// driver の参照を落とした。相手は `inbox` の残りを読みきってから EOF を受ける。
    pub(super) closed: bool,
    /// 書く側を閉じた。相手は `inbox` の残りを読みきってから EOF を受け、この端の書きは `BrokenPipe` になる。
    pub(super) shut: bool,
    /// 接続した側の端が持つポート。閉じると放す。
    port: Option<u16>,
}

struct Listener {
    port: u16,
    /// まだ受けていない接続の、受ける側の端。
    backlog: VecDeque<IoHandle>,
}

pub(super) struct Network {
    next_handle: IoHandle,
    pub(super) ends: HashMap<IoHandle, End>,
    listeners: HashMap<IoHandle, Listener>,
    /// ポートから、そこで待ち受けているリスナーへ。
    listening: HashMap<u16, IoHandle>,
    /// リスナーか接続した端が持つポート。
    used: HashSet<u16>,
    /// 次に振るポート。端まで行ったら戻る。
    next_port: u16,
}

impl Network {
    pub(super) fn new() -> Network {
        Network {
            next_handle: 0,
            ends: HashMap::new(),
            listeners: HashMap::new(),
            listening: HashMap::new(),
            used: HashSet::new(),
            next_port: FIRST_PORT,
        }
    }

    fn handle(&mut self) -> IoHandle {
        let handle = self.next_handle;
        self.next_handle += 1;
        handle
    }

    /// 使っていない次のポートを取る。すべて使っていれば `None` である。
    fn free_port(&mut self) -> Option<u16> {
        for _ in FIRST_PORT..=LAST_PORT {
            let port = self.next_port;
            self.next_port = if port == LAST_PORT {
                FIRST_PORT
            } else {
                port + 1
            };
            if self.used.insert(port) {
                return Some(port);
            }
        }
        None
    }

    /// つながった2つの端を作る。`port` は1つ目の端が持つポートである。
    fn connected_ends(&mut self, port: u16) -> (IoHandle, IoHandle) {
        let (a, b) = (self.handle(), self.handle());
        for (handle, peer, port) in [(a, b, Some(port)), (b, a, None)] {
            let end = End {
                peer,
                inbox: VecDeque::new(),
                closed: false,
                shut: false,
                port,
            };
            self.ends.insert(handle, end);
        }
        (a, b)
    }

    /// `port` で待ち受ける。0 なら空いているポートを振る。
    pub(super) fn listen(&mut self, port: u16) -> io::Result<Done> {
        let port = match port {
            0 => self
                .free_port()
                .ok_or_else(|| io::Error::new(io::ErrorKind::AddrInUse, "no free port"))?,
            port if self.used.insert(port) => port,
            _ => return Err(io::Error::from(io::ErrorKind::AddrInUse)),
        };
        let handle = self.handle();
        let listener = Listener {
            port,
            backlog: VecDeque::new(),
        };
        self.listeners.insert(handle, listener);
        self.listening.insert(port, handle);
        Ok(Done::Listener { handle, port })
    }

    /// `port` のリスナーへ接続する。接続する側の端にはポートを振る。
    pub(crate) fn virtual_connect(&mut self, port: u16) -> io::Result<Connection> {
        let Some(&listener) = self.listening.get(&port) else {
            return Ok(Connection::Refused);
        };
        if self.listeners[&listener].backlog.len() >= BACKLOG {
            return Ok(Connection::Pending);
        }
        let client_port = self
            .free_port()
            .ok_or_else(|| io::Error::other("no free port"))?;
        let (client_end, server_end) = self.connected_ends(client_port);
        let listener = self.listeners.get_mut(&listener).expect("a listening port");
        listener.backlog.push_back(server_end);
        Ok(Connection::Connected { client_end })
    }

    /// backlog から接続を1つ受け、受ける側の端と接続した側のポートを返す。backlog が空なら `None` である。
    pub(super) fn accept(&mut self, listener: IoHandle) -> Option<io::Result<(IoHandle, u16)>> {
        let Some(listener) = self.listeners.get_mut(&listener) else {
            return Some(Err(no_handle()));
        };
        let server_end = listener.backlog.pop_front()?;
        let peer = self.ends[&server_end].peer;
        let port = self.ends[&peer].port.expect("a connecting end has a port");
        Some(Ok((server_end, port)))
    }

    pub(super) fn shutdown_write(&mut self, handle: IoHandle) -> io::Result<()> {
        let end = self.ends.get_mut(&handle).ok_or_else(no_handle)?;
        end.shut = true;
        Ok(())
    }

    pub(super) fn is_stream(&self, handle: IoHandle) -> bool {
        self.ends.contains_key(&handle)
    }

    /// driver の参照を落とす。リスナーを閉じると、まだ受けていない接続の端も閉じ、接続した側は EOF を読む。
    pub(super) fn close(&mut self, handle: IoHandle) {
        if let Some(listener) = self.listeners.get_mut(&handle) {
            let backlog = std::mem::take(&mut listener.backlog);
            let port = listener.port;
            if self.listening.get(&port) == Some(&handle) {
                self.listening.remove(&port);
                self.used.remove(&port);
            }
            for end in backlog {
                self.close(end);
            }
            return;
        }
        if let Some(end) = self.ends.get_mut(&handle)
            && !end.closed
        {
            end.closed = true;
            if let Some(port) = end.port {
                self.used.remove(&port);
            }
        }
    }
}

pub(super) fn no_handle() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "no such handle")
}
