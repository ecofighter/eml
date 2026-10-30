//! テスト専用の証明書と鍵。何も認証しない。
//!
//! 鍵は公開された定数で、`eml` の実行ファイルにも入る (`Machine` が組むテスト用の driver から届く)。
//! この CA を信頼するのは、`ca.pem` を `Tls.connect_with_ca` か `Tls.wrap_with_ca` に明示して渡したプログラムだけである。
//! 作り直すには `fixtures/gen.sh` を流す。

pub const CA_PEM: &[u8] = include_bytes!("../fixtures/ca.pem");
pub const CERT_PEM: &[u8] = include_bytes!("../fixtures/cert.pem");
pub const KEY_PEM: &[u8] = include_bytes!("../fixtures/key.pem");
