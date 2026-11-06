//! `Std.Tls` の extern の腕 (docs/spec/stdlib.md の「`Std.Tls`」)。TLS の状態は `eml_tls::Conn` が持ち、それを `lin`
//! の `Foreign` に入れる。ソケットの I/O は `std/Tls.em` が `Std.Net` で行うので、ここには待ちも driver もない。
//!
//! feature `tls`を外した処理系では、`Conn`を値のない型にし、`client_conn`がいつも`not available`で失敗する。値を
//! 作れないので、ほかの腕には届かない。`vm.rs`にcfgを置かないため、両方で同じ名前とメソッドを使う。

use eml_rt::externs;
use eml_rt::{Ctx, Mode, Word};

use crate::cells::{self, Found, LINE_LIMIT, READ_ROOM};
use crate::driver::DriverKind;
use crate::error::VmFault;

#[cfg(feature = "tls")]
use eml_tls::{Conn, PEER_CLOSED};
#[cfg(not(feature = "tls"))]
use off::{Conn, PEER_CLOSED};

/// TLSの接続の設定の誤りの種類 (docs/spec/stdlib.mdの「`Std.Tls`」)。
#[cfg(feature = "tls")]
pub(crate) const SETUP_KIND: i64 = externs::IO_OTHER;
#[cfg(not(feature = "tls"))]
pub(crate) const SETUP_KIND: i64 = externs::IO_NOT_AVAILABLE;

#[cfg(not(feature = "tls"))]
mod off {
    /// TLSのない処理系の`Conn`。値を持たない。
    pub(crate) enum Conn {}

    pub(crate) const PEER_CLOSED: u32 = 4;

    impl Conn {
        pub(crate) fn push_tls(&mut self, _: &[u8]) {
            match *self {}
        }

        pub(crate) fn push_eof(&mut self) {
            match *self {}
        }

        pub(crate) fn pull_tls(&mut self, _: &mut Vec<u8>) {
            match *self {}
        }

        pub(crate) fn write_plain(&mut self, _: &[u8]) {
            match *self {}
        }

        pub(crate) fn plaintext(&self) -> &[u8] {
            match *self {}
        }

        pub(crate) fn consume(&mut self, _: usize) {
            match *self {}
        }

        pub(crate) fn status(&self) -> u32 {
            match *self {}
        }

        pub(crate) fn error_message(&self) -> String {
            match *self {}
        }

        pub(crate) fn send_close_notify(&mut self) {
            match *self {}
        }
    }
}

/// `Std.Tls.Conn` の `Foreign` の中身。`line_from` は行を探し終えた位置で、`plaintext()[..line_from]` に `\n` はない。
/// 平文を返すのは `TlsConn::consume` だけで、そこで 0 に戻す。`push_tls` は平文の後ろに足すだけなので、この関係を
/// 崩さず、次の `line_ready` は新しく届いたバイトだけを探せる。
pub(crate) struct TlsConn {
    conn: Conn,
    line_from: usize,
}

impl TlsConn {
    fn consume(&mut self, n: usize) {
        self.conn.consume(n);
        self.line_from = 0;
    }
}

/// 平文の先頭の行の状態。
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Line {
    /// 行も EOF もまだない。
    More,
    /// 終わりの改行を含めた行のバイト数。close_notify の後で残りがなければ 0 である。
    Ready(usize),
    /// 行の中身が `LINE_LIMIT` バイトを超えた。
    TooLong,
    /// 行が UTF-8 でない。
    NotUtf8,
}

/// `pending` の `from` バイト目から後を探し、最初の行の状態と、次に探し始める位置を返す。切り出しと上限は
/// `Net.read_line` と同じ関数で決める (docs/spec/stdlib.md の「`Std.Net`」)。
pub(crate) fn line_state(pending: &[u8], from: usize, eof: bool) -> (Line, usize) {
    match cells::find_line(pending, from, eof) {
        Some(Found::Line(n)) => {
            let line = if std::str::from_utf8(&pending[..n]).is_ok() {
                Line::Ready(n)
            } else {
                Line::NotUtf8
            };
            (line, from)
        }
        Some(Found::TooLong) => (Line::TooLong, from),
        None => {
            let line = if cells::open_line_len(pending) > LINE_LIMIT {
                Line::TooLong
            } else {
                Line::More
            };
            (line, pending.len())
        }
    }
}

/// `Foreign` の `conn` の中身。返すポインタは、`conn` の参照を手放すまでの間だけ使える。
///
/// # Safety
/// `conn` は呼ぶ側が持つ `Std.Tls.Conn` のただ1つの参照である (`Conn` は `Lin`)。
unsafe fn tls_conn<M: Mode>(ctx: &Ctx<M>, conn: Word) -> Result<*mut TlsConn, VmFault> {
    Ok(unsafe { ctx.heap().foreign_unique::<TlsConn>(conn) }?)
}

/// runの接続が信頼する根。runのdriverの種類からworkerの上で選ぶ (docs/spec/vm.mdの「時計と driver」)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Roots {
    /// `Machine`が自分で作ったテストのdriver: 渡したCAだけを根にし、`fixtures::NOW`の時刻で確かめる。
    Fixtures,
    /// 既定の根に、渡したCAを足したもの。
    Platform,
}

impl Roots {
    pub(crate) fn of(kind: DriverKind) -> Roots {
        if kind == DriverKind::Virtual {
            Roots::Fixtures
        } else {
            Roots::Platform
        }
    }

    #[cfg(feature = "tls")]
    fn trust(self, pem: &[u8]) -> Result<eml_tls::Trust, eml_tls::Error> {
        match self {
            Roots::Fixtures => eml_tls::Trust::fixtures(pem),
            Roots::Platform => eml_tls::Trust::platform(pem),
        }
    }
}

/// クライアントの`Foreign`の中身を作る。`Ctx`には触れない。誤りはサーバーの名前、信頼、設定の順に確かめる。
#[cfg(feature = "tls")]
pub(crate) fn client_conn(roots: Roots, host: &str, pem: &[u8]) -> Result<TlsConn, String> {
    eml_tls::Name::parse(host)
        .and_then(|name| {
            roots
                .trust(pem)
                .and_then(|trust| Conn::client(&name, &trust))
        })
        .map(|conn| TlsConn { conn, line_from: 0 })
        .map_err(|e| format!("cannot set up TLS for {host}: {e}"))
}

/// クライアントの`Foreign`の中身を作る。`Ctx`には触れない。誤りはサーバーの名前、信頼、設定の順に確かめる。
#[cfg(not(feature = "tls"))]
pub(crate) fn client_conn(roots: Roots, host: &str, pem: &[u8]) -> Result<TlsConn, String> {
    let _ = (roots, pem);
    Err(format!("cannot set up TLS for {host}: not available"))
}

/// `Std.Tls.push_tls`。`bytes` は借り、`conn` を消費して返す。
///
/// # Safety
/// `bytes` は生きた `Bytes` で、`conn` は `tls_conn` の前提を守る。
pub(crate) unsafe fn push_tls<M: Mode>(
    ctx: &mut Ctx<M>,
    bytes: Word,
    conn: Word,
) -> Result<Word, VmFault> {
    // バイト列と `Foreign` は別の物体で、`push_tls` はヒープに触れない
    let b = unsafe { &*ctx.heap().bytes(bytes)? };
    let c = unsafe { &mut *tls_conn(ctx, conn)? };
    c.conn.push_tls(b);
    Ok(conn)
}

/// `Std.Tls.write_plain`。`bytes` は借り、`conn` を消費して返す。
///
/// # Safety
/// `push_tls` と同じ。
pub(crate) unsafe fn write_plain<M: Mode>(
    ctx: &mut Ctx<M>,
    bytes: Word,
    conn: Word,
) -> Result<Word, VmFault> {
    let b = unsafe { &*ctx.heap().bytes(bytes)? };
    let c = unsafe { &mut *tls_conn(ctx, conn)? };
    c.conn.write_plain(b);
    Ok(conn)
}

/// `Std.Tls.push_eof`。
///
/// # Safety
/// `conn` は `tls_conn` の前提を守る。
pub(crate) unsafe fn push_eof<M: Mode>(ctx: &mut Ctx<M>, conn: Word) -> Result<Word, VmFault> {
    unsafe { (*tls_conn(ctx, conn)?).conn.push_eof() };
    Ok(conn)
}

/// `Std.Tls.send_close_notify`。
///
/// # Safety
/// `conn` は `tls_conn` の前提を守る。
pub(crate) unsafe fn send_close_notify<M: Mode>(
    ctx: &mut Ctx<M>,
    conn: Word,
) -> Result<Word, VmFault> {
    unsafe { (*tls_conn(ctx, conn)?).conn.send_close_notify() };
    Ok(conn)
}

/// `Std.Tls.pull_tls`。`conn` を借り、送るべき暗号文をすべて新しい `Bytes` にする。
///
/// # Safety
/// `conn` は `tls_conn` の前提を守る。
pub(crate) unsafe fn pull_tls<M: Mode>(ctx: &mut Ctx<M>, conn: Word) -> Result<Word, VmFault> {
    let mut out = Vec::new();
    unsafe { (*tls_conn(ctx, conn)?).conn.pull_tls(&mut out) };
    Ok(unsafe { ctx.heap_mut() }.alloc_bytes(&out))
}

/// `Std.Tls.read_plain`。`buf` を消費し、平文の先頭の `READ_ROOM` バイトまでを後ろに足して返す。`conn` は借りる。
///
/// # Safety
/// `buf` は呼ぶ側が持つ `Bytes` の参照で、`conn` は `tls_conn` の前提を守る。
pub(crate) unsafe fn read_plain<M: Mode>(
    ctx: &mut Ctx<M>,
    buf: Word,
    conn: Word,
) -> Result<Word, VmFault> {
    let c = unsafe { &mut *tls_conn(ctx, conn)? };
    let n = c.conn.plaintext().len().min(READ_ROOM);
    if n == 0 {
        return Ok(buf);
    }
    // 写すバイトは `Conn` の `Vec` の中にあり、ヒープの物体と重ならない
    let buf = unsafe { ctx.heap_mut().bytes_extend(buf, &c.conn.plaintext()[..n])? };
    c.consume(n);
    Ok(buf)
}

/// `Std.Tls.status`。`conn` を借りる。
///
/// # Safety
/// `conn` は `tls_conn` の前提を守る。
pub(crate) unsafe fn status<M: Mode>(ctx: &mut Ctx<M>, conn: Word) -> Result<i64, VmFault> {
    Ok(i64::from(unsafe { (*tls_conn(ctx, conn)?).conn.status() }))
}

/// `Std.Tls.error_message`。`conn` を借りる。
///
/// # Safety
/// `conn` は `tls_conn` の前提を守る。
pub(crate) unsafe fn error_message<M: Mode>(ctx: &mut Ctx<M>, conn: Word) -> Result<Word, VmFault> {
    let message = unsafe { (*tls_conn(ctx, conn)?).conn.error_message() };
    Ok(ctx.fresh_string(&message))
}

/// `TlsConn` の平文の先頭の行の状態。close_notify を受けた後だけを EOF とみなす。close_notify のない EOF は
/// `std/Tls.em` が失敗にする (docs/spec/stdlib.md の「`Std.Tls`」)。
fn ready_line(c: &mut TlsConn) -> Line {
    let eof = c.conn.status() & PEER_CLOSED != 0;
    let (line, from) = line_state(c.conn.plaintext(), c.line_from, eof);
    c.line_from = from;
    line
}

/// `Std.Tls.line_ready`。`conn` を借り、0 (まだ)、1 (行がある)、2 (長すぎる)、3 (UTF-8 でない) を返す。
///
/// # Safety
/// `conn` は `tls_conn` の前提を守る。
pub(crate) unsafe fn line_ready<M: Mode>(ctx: &mut Ctx<M>, conn: Word) -> Result<i64, VmFault> {
    let c = unsafe { &mut *tls_conn(ctx, conn)? };
    Ok(match ready_line(c) {
        Line::More => 0,
        Line::Ready(_) => 1,
        Line::TooLong => 2,
        Line::NotUtf8 => 3,
    })
}

/// `Std.Tls.take_line`。`conn` を借り、`line_ready` が 1 を返した行を終わりの改行ごと新しい文字列にして返す。
///
/// # Safety
/// `conn` は `tls_conn` の前提を守る。
pub(crate) unsafe fn take_line<M: Mode>(ctx: &mut Ctx<M>, conn: Word) -> Result<Word, VmFault> {
    let c = unsafe { &mut *tls_conn(ctx, conn)? };
    let Line::Ready(n) = ready_line(c) else {
        return Err(VmFault::Internal(
            "Std.Tls.take_line runs without a ready line",
        ));
    };
    // `line_state` が UTF-8 であることを確かめた
    let text = std::str::from_utf8(&c.conn.plaintext()[..n])
        .map_err(|_| VmFault::Internal("Std.Tls.take_line found a line that is not UTF-8"))?;
    let w = ctx.fresh_string(text);
    c.consume(n);
    Ok(w)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_end_with_lf_or_crlf() {
        assert_eq!(line_state(b"ab\ncd", 0, false), (Line::Ready(3), 0));
        assert_eq!(line_state(b"ab\r\ncd", 0, false), (Line::Ready(4), 0));
    }

    #[test]
    fn no_newline_is_more_and_moves_from() {
        assert_eq!(line_state(b"abc", 0, false), (Line::More, 3));
    }

    #[test]
    fn eof_takes_the_rest() {
        assert_eq!(line_state(b"abc", 3, true), (Line::Ready(3), 3));
        assert_eq!(line_state(b"", 0, true), (Line::Ready(0), 0));
    }

    /// 上限は行の中身の長さで決め、終わりの改行は数えない (docs/spec/stdlib.md の「`Std.Net`」)。
    #[test]
    fn the_limit_counts_content() {
        let mut crlf = vec![b'a'; LINE_LIMIT];
        crlf.extend_from_slice(b"\r\n");
        assert_eq!(line_state(&crlf, 0, false).0, Line::Ready(LINE_LIMIT + 2));
        let mut cr = vec![b'a'; LINE_LIMIT];
        cr.push(b'\r');
        assert_eq!(line_state(&cr, 0, false).0, Line::More);
        let long = vec![b'a'; LINE_LIMIT + 1];
        assert_eq!(line_state(&long, 0, false).0, Line::TooLong);
        let mut long_lf = long.clone();
        long_lf.push(b'\n');
        assert_eq!(line_state(&long_lf, 0, false).0, Line::TooLong);
    }

    #[test]
    fn invalid_utf8_is_reported() {
        assert_eq!(line_state(b"\xff\n", 0, false).0, Line::NotUtf8);
    }

    #[test]
    fn a_crlf_split_across_pushes_is_one_end() {
        assert_eq!(line_state(b"ab\r", 0, false), (Line::More, 3));
        assert_eq!(line_state(b"ab\r\nx", 3, false), (Line::Ready(4), 3));
    }

    /// 前の探しで進めた `from` から探しても、先頭から探したのと同じ行になる。
    #[test]
    fn an_incremental_from_matches_from_zero() {
        let input: &[u8] = b"one\r\ntwo\n\nthree\rfour\r\n";
        for split in 0..=input.len() {
            let (first, from) = line_state(&input[..split], 0, false);
            let from = if first == Line::More { from } else { 0 };
            assert_eq!(
                line_state(input, from, false).0,
                line_state(input, 0, false).0,
                "split at {split}"
            );
        }
    }
}

#[cfg(all(test, feature = "tls"))]
mod on_tests {
    use eml_rt::Checked;
    use eml_tls::HANDSHAKING;
    use eml_tls::fixtures::{CA_PEM, CERT_PEM, KEY_PEM};

    use super::*;

    fn new_client(ctx: &mut Ctx<Checked>) -> Word {
        let c = client_conn(Roots::Fixtures, "tls.localhost", CA_PEM);
        unsafe { ctx.heap_mut() }.alloc_foreign(c.ok().unwrap(), true)
    }

    /// クライアントの `Foreign` と server の間で、どちらも送るものがなくなるまで暗号文を汲む。
    fn pump(ctx: &mut Ctx<Checked>, client: Word, server: &mut Conn) {
        loop {
            let out = unsafe { pull_tls(ctx, client) }.unwrap();
            let sent = unsafe { (*ctx.heap().bytes(out).unwrap()).to_vec() };
            unsafe { ctx.heap_mut().decref(out) }.unwrap();
            server.push_tls(&sent);
            let mut back = Vec::new();
            server.pull_tls(&mut back);
            if sent.is_empty() && back.is_empty() {
                return;
            }
            // handshakeの間は`Conn`に直に押し込み、その後は`push_tls`の腕で押し込む。handshakeの後の押し込みで
            // 腕を通すためである
            if unsafe { status(ctx, client) }.unwrap() & i64::from(HANDSHAKING) != 0 {
                unsafe { (*tls_conn(ctx, client).unwrap()).conn.push_tls(&back) };
            } else {
                let b = unsafe { ctx.heap_mut() }.alloc_bytes(&back);
                unsafe { push_tls(ctx, b, client) }.unwrap();
                unsafe { ctx.heap_mut().decref(b) }.unwrap();
            }
        }
    }

    fn connected(ctx: &mut Ctx<Checked>) -> (Word, Conn) {
        let client = new_client(ctx);
        let mut server = Conn::server(CERT_PEM, KEY_PEM).unwrap();
        pump(ctx, client, &mut server);
        assert_eq!(unsafe { status(ctx, client) }.unwrap(), 0);
        (client, server)
    }

    fn read(ctx: &mut Ctx<Checked>, client: Word) -> Vec<u8> {
        let empty = unsafe { ctx.heap_mut() }.alloc_bytes(b"");
        let got = unsafe { read_plain(ctx, empty, client) }.unwrap();
        let bytes = unsafe { (*ctx.heap().bytes(got).unwrap()).to_vec() };
        unsafe { ctx.heap_mut().decref(got) }.unwrap();
        bytes
    }

    /// `read_plain` と行の読みを混ぜても、バイトは失われず順も変わらない。`read_plain` は1回に `READ_ROOM` バイト
    /// までを返す。
    #[test]
    fn read_plain_and_lines_interleave() {
        let mut ctx = cells::test_ctx();
        let (client, mut server) = connected(&mut ctx);

        server.write_plain(b"ab");
        pump(&mut ctx, client, &mut server);
        assert_eq!(unsafe { line_ready(&mut ctx, client) }.unwrap(), 0);
        assert_eq!(read(&mut ctx, client), b"ab");

        server.write_plain(b"cd\nef");
        pump(&mut ctx, client, &mut server);
        assert_eq!(unsafe { line_ready(&mut ctx, client) }.unwrap(), 1);
        let line = unsafe { take_line(&mut ctx, client) }.unwrap();
        assert_eq!(
            unsafe { ctx.copy_string(line, usize::MAX) }.unwrap(),
            "cd\n"
        );

        server.write_plain(&vec![b'x'; 100_000]);
        pump(&mut ctx, client, &mut server);
        let chunk = read(&mut ctx, client);
        assert_eq!(chunk.len(), READ_ROOM);
        assert!(chunk.starts_with(b"efxx"));

        unsafe { ctx.heap_mut().decref(client) }.unwrap();
        let leaks = ctx.leaks();
        assert!(leaks.is_empty(), "{leaks:?}");
    }

    #[test]
    fn pull_tls_on_an_idle_conn_is_empty() {
        let mut ctx = cells::test_ctx();
        let (client, _server) = connected(&mut ctx);
        let out = unsafe { pull_tls(&mut ctx, client) }.unwrap();
        assert!(unsafe { &*ctx.heap().bytes(out).unwrap() }.is_empty());
        unsafe { ctx.heap_mut().decref(out) }.unwrap();
        unsafe { ctx.heap_mut().decref(client) }.unwrap();
        let leaks = ctx.leaks();
        assert!(leaks.is_empty(), "{leaks:?}");
    }

    /// 設定の誤りの文言は固定である。
    #[test]
    fn client_errors_are_other_with_fixed_messages() {
        for (host, pem, message) in [
            (
                "a b",
                "",
                "cannot set up TLS for a b: the server name is not valid",
            ),
            (
                "tls.localhost",
                "garbage",
                "cannot set up TLS for tls.localhost: the CA certificates are not valid PEM",
            ),
            (
                "tls.localhost",
                "",
                "cannot set up TLS for tls.localhost: no root certificates are available",
            ),
        ] {
            let Err(got) = client_conn(Roots::Fixtures, host, pem.as_bytes()) else {
                panic!("{host} with {pem:?} did not fail");
            };
            assert_eq!(got, message);
        }
    }
}
