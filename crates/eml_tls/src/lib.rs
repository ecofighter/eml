//! rustls の接続を、I/O を持たない形で包む。
//!
//! eml のどの crate にも依存しない。バイトを汲む側 (`eml_vm` と `std/`) が I/O を受け持つ。
//! 内部の決まりは docs/implementation/architecture.md の「`eml_tls` の内部」にある。

use std::fmt;
use std::io::{self, BufRead};
use std::sync::Arc;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore, ServerConfig, ServerConnection};

pub mod fixtures;

/// handshake の途中である
pub const HANDSHAKING: u32 = 1;
/// `pull_tls` で出すバイトがある
pub const HAS_OUTGOING: u32 = 2;
/// 相手の close_notify を受けた
pub const PEER_CLOSED: u32 = 4;
/// close_notify なしに EOF が来た (handshake の途中も含む)
pub const TRUNCATED: u32 = 8;
/// 失敗を記録した
pub const FAILED: u32 = 16;

/// 接続を作るときの誤り。文言は固定である
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    ServerName,
    CaPem,
    CertPem,
    KeyPem,
    Config,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Error::ServerName => "the server name is not valid",
            Error::CaPem => "the CA certificates are not valid PEM",
            Error::CertPem => "the certificate chain is not valid PEM",
            Error::KeyPem => "the private key is not valid PEM",
            Error::Config => "the TLS configuration is not valid",
        })
    }
}

impl std::error::Error for Error {}

enum Failure {
    // 証明書の拒否は理由を問わず同じ文言にする (docs/implementation/architecture.md の「`eml_tls` の内部」)
    Rejected,
    Tls(rustls::Error),
    Io(io::Error),
}

pub struct Conn {
    tls: rustls::Connection,
    /// 読んでいない平文は `plain[start..]`
    plain: Vec<u8>,
    start: usize,
    failure: Option<Failure>,
    eof: bool,
    peer_closed: bool,
}

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::aws_lc_rs::default_provider())
}

impl Conn {
    /// `extra_ca_pem` の証明書を webpki-roots の根に足して信頼する。空なら根だけである
    pub fn client(server_name: &str, extra_ca_pem: &[u8]) -> Result<Conn, Error> {
        let name = ServerName::try_from(server_name.to_owned()).map_err(|_| Error::ServerName)?;
        let mut roots = RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let mut added = 0;
        for der in CertificateDer::pem_slice_iter(extra_ca_pem) {
            let der = der.map_err(|_| Error::CaPem)?;
            roots.add(der).map_err(|_| Error::CaPem)?;
            added += 1;
        }
        if !extra_ca_pem.is_empty() && added == 0 {
            return Err(Error::CaPem);
        }
        let config = ClientConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()
            .map_err(|_| Error::Config)?
            .with_root_certificates(roots)
            .with_no_client_auth();
        let tls = ClientConnection::new(Arc::new(config), name).map_err(|_| Error::Config)?;
        Ok(Conn::new(tls.into()))
    }

    /// テストの driver の終端器とテストのためだけにある
    pub fn server(cert_chain_pem: &[u8], key_pem: &[u8]) -> Result<Conn, Error> {
        let chain: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(cert_chain_pem)
            .collect::<Result<_, _>>()
            .map_err(|_| Error::CertPem)?;
        if chain.is_empty() {
            return Err(Error::CertPem);
        }
        let key = PrivateKeyDer::from_pem_slice(key_pem).map_err(|_| Error::KeyPem)?;
        let config = ServerConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()
            .map_err(|_| Error::Config)?
            .with_no_client_auth()
            .with_single_cert(chain, key)
            .map_err(|_| Error::KeyPem)?;
        let tls = ServerConnection::new(Arc::new(config)).map_err(|_| Error::Config)?;
        Ok(Conn::new(tls.into()))
    }

    fn new(mut tls: rustls::Connection) -> Conn {
        // 上限があると `write_plain` が平文の一部だけを受け取りうる
        // (docs/implementation/architecture.md の「`eml_tls` の内部」)
        tls.set_buffer_limit(None);
        Conn {
            tls,
            plain: Vec::new(),
            start: 0,
            failure: None,
            eof: false,
            peer_closed: false,
        }
    }

    /// 相手から届いた暗号文を渡す。失敗の後と close_notify の後は何も読まない
    pub fn push_tls(&mut self, mut bytes: &[u8]) {
        while !bytes.is_empty() && self.failure.is_none() && !self.peer_closed {
            match self.tls.read_tls(&mut bytes) {
                Ok(0) => break,
                Ok(_) => {}
                Err(e) => {
                    self.failure = Some(Failure::Io(e));
                    break;
                }
            }
            // 受けた平文の rustls の buffer は `set_buffer_limit` によらず上限を持ち、満ちると
            // `read_tls` が失敗する。だから処理のたびに自分の列へ移す
            // (docs/implementation/architecture.md の「`eml_tls` の内部」)
            match self.tls.process_new_packets() {
                Ok(state) => {
                    self.drain();
                    if state.peer_has_closed() {
                        self.peer_closed = true;
                    }
                }
                Err(e) => {
                    self.drain();
                    self.failure = Some(match e {
                        rustls::Error::InvalidCertificate(_) => Failure::Rejected,
                        e => Failure::Tls(e),
                    });
                }
            }
        }
    }

    // 詰め直しても `plaintext()` の中身は変わらないので、その中の位置は押し込みをまたいで保たれる
    fn drain(&mut self) {
        self.plain.drain(..self.start);
        self.start = 0;
        let mut reader = self.tls.reader();
        loop {
            match reader.fill_buf() {
                Ok(chunk) if !chunk.is_empty() => {
                    let n = chunk.len();
                    self.plain.extend_from_slice(chunk);
                    reader.consume(n);
                }
                _ => break,
            }
        }
    }

    /// 相手の EOF を知らせる。rustls には渡さず、状態の印にだけ表れる
    pub fn push_eof(&mut self) {
        self.eof = true;
    }

    /// 出すべき暗号文をすべて `out` に足す。失敗の後の alert もここから出る
    pub fn pull_tls(&mut self, out: &mut Vec<u8>) {
        while self.tls.wants_write() {
            // `Vec` への書き込みは失敗しない
            let _ = self.tls.write_tls(out);
        }
    }

    /// 平文を暗号化して送る列に積む。失敗の後は捨てる
    pub fn write_plain(&mut self, bytes: &[u8]) {
        if self.failure.is_some() {
            return;
        }
        // 上限を外したので、書き込みは全部を受け取る
        let _ = io::Write::write_all(&mut self.tls.writer(), bytes);
    }

    /// 読んでいない平文
    pub fn plaintext(&self) -> &[u8] {
        &self.plain[self.start..]
    }

    /// 読んでいない平文の先頭 `n` バイトを読んだことにする
    pub fn consume(&mut self, n: usize) {
        self.start += n;
        assert!(self.start <= self.plain.len());
        if self.start == self.plain.len() {
            self.plain = Vec::new();
            self.start = 0;
        }
    }

    /// 読んでいない平文をすべて `out` に足し、その長さを返す
    pub fn read_plain(&mut self, out: &mut Vec<u8>) -> usize {
        let n = self.plaintext().len();
        out.extend_from_slice(self.plaintext());
        self.consume(n);
        n
    }

    pub fn status(&self) -> u32 {
        let mut status = 0;
        if self.tls.is_handshaking() {
            status |= HANDSHAKING;
        }
        if self.tls.wants_write() {
            status |= HAS_OUTGOING;
        }
        if self.peer_closed {
            status |= PEER_CLOSED;
        }
        if self.eof && !self.peer_closed {
            status |= TRUNCATED;
        }
        if self.failure.is_some() {
            status |= FAILED;
        }
        status
    }

    /// 失敗の文言。失敗していなければ空である
    pub fn error_message(&self) -> String {
        match &self.failure {
            None => String::new(),
            Some(Failure::Rejected) => "the server certificate was rejected".to_owned(),
            Some(Failure::Tls(e)) => e.to_string(),
            Some(Failure::Io(e)) => e.to_string(),
        }
    }

    /// 何度呼んでも1度だけ送る。致命的な alert を送った後は何もしない
    pub fn send_close_notify(&mut self) {
        self.tls.send_close_notify();
    }
}
