//! テストと計測の相手のための証明書と鍵。何も認証しない。
//!
//! 鍵は公開された定数で、`eml` の実行ファイルにも入る (`Machine` が組むテスト用の driver から届く)。
//! この CA を信頼するのは、`ca.pem` を `Tls.connect_with_ca` か `Tls.wrap_with_ca` に明示して渡したプログラムだけである。
//! 作り直すには `fixtures/gen.sh` を流す。
//!
//! handshakeの成功を期待するテストは`NOW`の時刻で確かめるので、結果は日付に左右されない。現実の時刻では、証明書は
//! 期限切れである。

use std::time::Duration;

use rustls::pki_types::UnixTime;

pub const CA_PEM: &[u8] = include_bytes!("../fixtures/ca.pem");
pub const CERT_PEM: &[u8] = include_bytes!("../fixtures/cert.pem");
pub const KEY_PEM: &[u8] = include_bytes!("../fixtures/key.pem");
/// 何にも署名していない2つ目のCA。テストのdriverの証明書が拒まれることを期待するテストは、このCAだけを信頼する
pub const OTHER_CA_PEM: &[u8] = include_bytes!("../fixtures/other-ca.pem");

/// 証明書の有効期間の中の時刻 (2026-01-15T00:00:00Z)
pub const NOW: UnixTime = UnixTime::since_unix_epoch(Duration::from_secs(1_768_435_200));
