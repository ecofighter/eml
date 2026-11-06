//! rustls の接続を、I/O を持たない形で包む。
//!
//! eml のどの crate にも依存しない。バイトを汲む側 (`eml_vm` と `std/`) が I/O を受け持つ。
//! 内部の決まりは docs/implementation/architecture.md の「`eml_tls` の内部」にある。

use std::fmt;
use std::io::{self, BufRead};
use std::sync::Arc;

use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::time_provider::TimeProvider;
use rustls::{
    CertificateError, ClientConfig, ClientConnection, DigitallySignedStruct, DistinguishedName,
    RootCertStore, ServerConfig, ServerConnection, SignatureScheme,
};

pub use rustls::pki_types::UnixTime;

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
    NoRoots,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Error::ServerName => "the server name is not valid",
            Error::CaPem => "the CA certificates are not valid PEM",
            Error::CertPem => "the certificate chain is not valid PEM",
            Error::KeyPem => "the private key is not valid PEM",
            Error::Config => "the TLS configuration is not valid",
            Error::NoRoots => "no root certificates are available",
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

/// SNIと証明書の名前の照合に使う名前
#[derive(Clone, Debug)]
pub struct Name(ServerName<'static>);

impl Name {
    pub fn parse(name: &str) -> Result<Name, Error> {
        ServerName::try_from(name.to_owned())
            .map(Name)
            .map_err(|_| Error::ServerName)
    }
}

/// クライアントがサーバーの証明書を確かめる方法。ブロックしてよい所で作れるよう、`Conn::client`と分けて作る。複製は安く、
/// 別のスレッドに送れる
#[derive(Clone, Debug)]
pub struct Trust {
    verifier: Arc<dyn ServerCertVerifier>,
    now: Option<UnixTime>,
}

impl Trust {
    /// OSの検証器で確かめ、OSの根に加えて`extra_ca_pem`のCAを信頼する。時刻は現実のものを使う。Linuxでは呼ぶたびに
    /// 根のファイルを読む
    pub fn platform(extra_ca_pem: &[u8]) -> Result<Trust, Error> {
        Trust::os(extra_ca_pem, None)
    }

    /// `platform`と同じだが、`now`の時刻で確かめる。OSの検証器のテストのためにある
    pub fn platform_at(extra_ca_pem: &[u8], now: UnixTime) -> Result<Trust, Error> {
        Trust::os(extra_ca_pem, Some(now))
    }

    /// テストの世界: `ca_pem`のCAだけを根にし、`fixtures::NOW`の時刻で確かめる。`ca_pem`が空なら`Error::NoRoots`である
    /// (docs/spec/vm.mdの「時計と driver」)
    pub fn fixtures(ca_pem: &[u8]) -> Result<Trust, Error> {
        let mut store = RootCertStore::empty();
        store.add_parsable_certificates(extra_roots(ca_pem)?);
        Trust::webpki(store, Some(fixtures::NOW))
    }

    fn os(extra_ca_pem: &[u8], now: Option<UnixTime>) -> Result<Trust, Error> {
        let roots = extra_roots(extra_ca_pem)?;
        let no_extra = roots.is_empty();
        // Linuxでは、システムの根も足す根もないと作るのに失敗する。その失敗を`Error::NoRoots`として報告する
        // (docs/spec/stdlib.mdの「`Std.Tls`」)
        let verifier = rustls_platform_verifier::Verifier::new_with_extra_roots(roots, provider())
            .map_err(|e| match e {
                _ if no_extra => Error::NoRoots,
                rustls::Error::InvalidCertificate(_) => Error::CaPem,
                _ => Error::Config,
            })?;
        Ok(Trust {
            verifier: Arc::new(Opaque(Arc::new(verifier))),
            now,
        })
    }

    fn webpki(store: RootCertStore, now: Option<UnixTime>) -> Result<Trust, Error> {
        if store.is_empty() {
            return Err(Error::NoRoots);
        }
        let verifier = WebPkiServerVerifier::builder_with_provider(Arc::new(store), provider())
            .build()
            .map_err(|_| Error::Config)?;
        Ok(Trust {
            verifier: Arc::new(Opaque(verifier)),
            now,
        })
    }
}

/// 検証器を作る前に足す根を確かめる。`Error::CaPem`を信頼のほかの誤りより先にし、検証器に左右されないようにするためである
fn extra_roots(pem: &[u8]) -> Result<Vec<CertificateDer<'static>>, Error> {
    let mut check = RootCertStore::empty();
    let mut roots = Vec::new();
    for der in CertificateDer::pem_slice_iter(pem) {
        let der = der.map_err(|_| Error::CaPem)?;
        check.add(der.clone()).map_err(|_| Error::CaPem)?;
        roots.push(der);
    }
    if !pem.is_empty() && roots.is_empty() {
        return Err(Error::CaPem);
    }
    Ok(roots)
}

#[derive(Debug)]
struct Pinned(UnixTime);

impl TimeProvider for Pinned {
    fn current_time(&self) -> Option<UnixTime> {
        Some(self.0)
    }
}

// 証明書の誤りでない検証器の誤りは、検証器の文言を含みうるので、理由のない拒否にする。証明書の誤りはそのまま通し、rustlsが
// 理由に合ったalertを送れるようにする (docs/spec/stdlib.mdの「`Std.Tls`」)
#[derive(Debug)]
struct Opaque(Arc<dyn ServerCertVerifier>);

impl ServerCertVerifier for Opaque {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        self.0
            .verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now)
            .map_err(|e| match e {
                e @ rustls::Error::InvalidCertificate(_) => e,
                _ => rustls::Error::InvalidCertificate(
                    CertificateError::ApplicationVerificationFailure,
                ),
            })
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.0.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.0.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.supported_verify_schemes()
    }

    fn requires_raw_public_keys(&self) -> bool {
        self.0.requires_raw_public_keys()
    }

    fn root_hint_subjects(&self) -> Option<&[DistinguishedName]> {
        self.0.root_hint_subjects()
    }
}

impl Conn {
    /// I/Oはしない。接続ごとに`ClientConfig`を作るので、ある接続がほかの接続のsessionを再開することはない
    pub fn client(name: &Name, trust: &Trust) -> Result<Conn, Error> {
        let builder = match trust.now {
            Some(now) => ClientConfig::builder_with_details(provider(), Arc::new(Pinned(now))),
            None => ClientConfig::builder_with_provider(provider()),
        };
        let config = builder
            .with_safe_default_protocol_versions()
            .map_err(|_| Error::Config)?
            .dangerous()
            .with_custom_certificate_verifier(trust.verifier.clone())
            .with_no_client_auth();
        let tls =
            ClientConnection::new(Arc::new(config), name.0.clone()).map_err(|_| Error::Config)?;
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
