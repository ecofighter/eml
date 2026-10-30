//! テストの driver の TLS の終端器 (docs/spec/vm.md の「時計と driver」)。`tls.localhost` と `tls-cut.localhost`
//! への接続ごとに、Rust の `eml_tls::Conn` のサーバーを置き、暗号文の端と平文の端の間でバイトを汲む。handshake
//! は乱数が毎回違って録音を再生できないので、決定的に動かせるのはここだけである。

use std::io;

use eml_rt::IoHandle;
use eml_tls::{Conn, FAILED, HANDSHAKING, PEER_CLOSED, TRUNCATED, fixtures};

use super::net::{Connection, Network};

/// 1回に暗号化する平文の上限。rustls の1レコードの大きさにそろえる。
const RECORD: usize = 16 * 1024;

/// 終端器を挟む名前なら、close_notify を送らずに閉じるか (`tls-cut.localhost`) を返す。
pub(super) fn tls_host(host: &str) -> Option<bool> {
    match host {
        "tls.localhost" => Some(false),
        "tls-cut.localhost" => Some(true),
        _ => None,
    }
}

/// 作った順に並ぶので、汲む順も決まる。
#[derive(Default)]
pub(super) struct Terminators(Vec<Terminator>);

struct Terminator {
    conn: Conn,
    /// 暗号文の端。相手はプログラムのクライアントの端である。
    cipher: IoHandle,
    /// 平文の対の接続した側の端。相手はプログラムのサーバーが受ける端である。
    plain: IoHandle,
    cut: bool,
    /// クライアントのバイトを1度でも `conn` に渡した。
    started: bool,
    eof_pushed: bool,
    stopped: bool,
}

impl Terminators {
    /// 平文の対をリスナーにつなぎ、暗号文の対の片方をクライアントの端として返す。拒まれた接続と待つ接続は
    /// ポートを取らず、そのまま返す。
    pub(super) fn connect(
        &mut self,
        net: &mut Network,
        port: u16,
        cut: bool,
    ) -> io::Result<Connection> {
        let plain = match net.virtual_connect(port)? {
            Connection::Connected { client_end } => client_end,
            other => return Ok(other),
        };
        let (client_end, cipher) = match net.pair() {
            Ok(pair) => pair,
            Err(e) => {
                net.close(plain);
                return Err(e);
            }
        };
        let conn =
            Conn::server(fixtures::CERT_PEM, fixtures::KEY_PEM).expect("the test certificate");
        self.0.push(Terminator {
            conn,
            cipher,
            plain,
            cut,
            started: false,
            eof_pushed: false,
            stopped: false,
        });
        Ok(Connection::Connected { client_end })
    }

    /// 終端器ごとに、動かなくなるまで汲む。終端器の平文の相手はいつもプログラムが持つ端で (リスナーを作るのは
    /// `Op::Listen` だけなので)、ある終端器が別の終端器にバイトを渡すことはない。そのため、終端器ごとの不動点が
    /// 全体の不動点になる。クライアントが自分の端を閉じた終端器は取り除く。
    pub(super) fn pump(&mut self, net: &mut Network) {
        self.0.retain_mut(|t| {
            while t.step(net) {}
            !net.ends[&t.cipher].closed
        });
    }
}

/// `to` が開いていれば `bytes` を届け、閉じていれば捨てる。
fn deliver(net: &mut Network, to: IoHandle, bytes: &[u8]) {
    let end = net.ends.get_mut(&to).expect("a terminator's peer");
    if !end.closed {
        end.inbox.extend(bytes);
    }
}

impl Terminator {
    fn handshaking(&self) -> bool {
        self.conn.status() & HANDSHAKING != 0
    }

    fn failed(&self) -> bool {
        self.conn.status() & FAILED != 0
    }

    fn peer_done(&self) -> bool {
        self.conn.status() & (PEER_CLOSED | TRUNCATED) != 0
    }

    /// 暗号文を `cipher` の相手に届ける。何か届けたかを返す。
    fn send(&mut self, net: &mut Network) -> bool {
        let mut out = Vec::new();
        self.conn.pull_tls(&mut out);
        let c = net.ends[&self.cipher].peer;
        deliver(net, c, &out);
        !out.is_empty()
    }

    /// 1回汲み、何か動いたかを返す。動いたことはどれも有限回しか起きないので、繰り返すと止まる。
    fn step(&mut self, net: &mut Network) -> bool {
        let c = net.ends[&self.cipher].peer;
        let s = net.ends[&self.plain].peer;
        // 止めた後もクライアントは書けるので、溜まらないように読んで捨てる
        let bytes: Vec<u8> = net
            .ends
            .get_mut(&self.cipher)
            .expect("a terminator's end")
            .inbox
            .drain(..)
            .collect();
        let mut moved = !bytes.is_empty();
        // クライアントのバイトを受ける前にサーバーが閉じたときだけ、TLS のバイトを送らずに EOF を見せる。
        // handshake の途中で閉じたときは、handshake を終えてから close_notify を送る。こうすると、受ける側が先に
        // 待っていれば、結果がタスクの順によらない
        if !self.stopped
            && !self.started
            && net.ends[&s].closed
            && net.ends[&self.plain].inbox.is_empty()
        {
            self.stop(net);
            return true;
        }
        if !self.stopped {
            moved |= self.relay(net, c, s, &bytes);
            if self.stopped {
                return true;
            }
        }
        if net.ends[&c].closed && !net.ends[&self.cipher].closed {
            net.close(self.plain);
            net.close(self.cipher);
            return false;
        }
        moved
    }

    /// 止めていない終端器の1回分。届いた暗号文 `bytes` を復号してサーバーへ、サーバーの平文を暗号化して
    /// クライアントへ渡す。
    fn relay(&mut self, net: &mut Network, c: IoHandle, s: IoHandle, bytes: &[u8]) -> bool {
        let mut moved = false;
        if !bytes.is_empty() {
            self.conn.push_tls(bytes);
            self.started = true;
        }
        let client = &net.ends[&c];
        if (client.closed || client.shut) && !self.eof_pushed {
            self.conn.push_eof();
            self.eof_pushed = true;
            moved = true;
        }
        if self.failed() {
            // alert を届けてから止める
            self.send(net);
            self.stop(net);
            return true;
        }
        let handshaking = self.handshaking();
        let mut plain = Vec::new();
        if self.conn.read_plain(&mut plain) > 0 {
            deliver(net, s, &plain);
            moved = true;
        }
        let plain_end = &net.ends[&self.plain];
        if (self.peer_done() || self.eof_pushed) && !plain_end.shut && !plain_end.closed {
            net.shutdown_write(self.plain).expect("a terminator's end");
            moved = true;
        }
        if !handshaking {
            let inbox = &mut net
                .ends
                .get_mut(&self.plain)
                .expect("a terminator's end")
                .inbox;
            let n = inbox.len().min(RECORD);
            if n > 0 {
                let chunk: Vec<u8> = inbox.drain(..n).collect();
                self.conn.write_plain(&chunk);
                moved = true;
            }
        }
        let server = &net.ends[&s];
        let cipher = &net.ends[&self.cipher];
        if !handshaking
            && (server.closed || server.shut)
            && net.ends[&self.plain].inbox.is_empty()
            && !cipher.shut
            && !cipher.closed
        {
            if !self.cut {
                self.conn.send_close_notify();
            }
            self.send(net);
            // 閉じずに書く側だけを閉じる。閉じると、EOF を読んだクライアントが書く close_notify が `BrokenPipe`
            // になる
            net.shutdown_write(self.cipher).expect("a terminator's end");
            moved = true;
        }
        moved |= self.send(net);
        if net.ends[&s].closed && !net.ends[&self.plain].closed {
            // ポートを放す
            net.close(self.plain);
            moved = true;
        }
        moved
    }

    /// 以後は汲まない。クライアントには EOF を見せるが書けるままにし、サーバーには残りを読ませてから EOF を
    /// 見せ、書きを `BrokenPipe` にする。
    fn stop(&mut self, net: &mut Network) {
        self.stopped = true;
        net.shutdown_write(self.cipher).expect("a terminator's end");
        net.close(self.plain);
    }
}
