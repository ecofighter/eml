# 付録 B: 行 `https-get` のソース

計画の T-3 と T-4 が入れるファイルの中身である。日本語のコメントは下書きなので、入れる前に natural-japanese の skill で直す。

## B.1 crates/eml_tls/examples/https_server.rs (T-3)

```rust
//! `bench/scripts.sh` の行 `https-get` の相手になる、ループバックの HTTPS の server
//! (docs/implementation/benchmarks.md の「スクリプトの計測」)。
//!
//! `https_server MIB` は `127.0.0.1` の空いたポートで待ち受け、ポートを標準出力の1行目に出す。どの要求にも
//! `Content-Length` の付いた MIB MiB の本文を返す。止めるのは呼ぶ側の signal である。鍵と証明書は `fixtures` の
//! テスト用のもので、何も認証しない。

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use eml_tls::fixtures::{CERT_PEM, KEY_PEM};
use rustls::crypto::CryptoProvider;
use rustls::crypto::aws_lc_rs::{cipher_suite, default_provider};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};

/// 本文の行の長さ。行の番号を15桁で書くので、最後の行から本文の長さと並びの崩れが読める
const LINE: usize = 16;

fn main() {
    let mib: usize = std::env::args()
        .nth(1)
        .and_then(|arg| arg.parse().ok())
        .expect("usage: https_server MIB");
    // 計測の間に本文を作らないよう、ポートを出す前に作っておく
    let mut body = Vec::with_capacity(mib << 20);
    for i in 0..(mib << 20) / LINE {
        writeln!(body, "{i:015}").unwrap();
    }
    let body: Arc<[u8]> = body.into();
    let chain = CertificateDer::pem_slice_iter(CERT_PEM)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let key = PrivateKeyDer::from_pem_slice(KEY_PEM).unwrap();
    // eml の rustls と Python の OpenSSL が別の暗号を選ぶと、比が暗号の速さの差を含むので、1つに固める
    let provider = CryptoProvider {
        cipher_suites: vec![cipher_suite::TLS13_AES_256_GCM_SHA384],
        ..default_provider()
    };
    let config = Arc::new(
        ServerConfig::builder_with_provider(Arc::new(provider))
            .with_protocol_versions(&[&rustls::version::TLS13])
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(chain, key)
            .unwrap(),
    );
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    println!("{}", listener.local_addr().unwrap().port());
    for tcp in listener.incoming() {
        let (config, body) = (config.clone(), body.clone());
        // クライアントの閉じ方を待つ間も、次の接続を受けられるよう、接続ごとにスレッドにする
        thread::spawn(move || {
            if let Err(e) = tcp.and_then(|tcp| serve(config, tcp, &body)) {
                eprintln!("https_server: {e}");
            }
        });
    }
}

fn serve(config: Arc<ServerConfig>, tcp: TcpStream, body: &[u8]) -> io::Result<()> {
    // 止まったクライアントのスレッドを残し続けない
    tcp.set_read_timeout(Some(Duration::from_secs(10)))?;
    let conn = ServerConnection::new(config).map_err(io::Error::other)?;
    let mut tls = StreamOwned::new(conn, tcp);
    let mut head = Vec::new();
    let mut buf = [0u8; 4096];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = tls.read(&mut buf)?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        head.extend_from_slice(&buf[..n]);
    }
    write!(
        tls,
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    for chunk in body.chunks(1 << 20) {
        tls.write_all(chunk)?;
    }
    tls.conn.send_close_notify();
    tls.flush()?;
    // 書く側だけを閉じる。EOF を待つクライアントはすぐ終わり、後から close_notify を送るクライアントも reset を
    // 受けない。読む側はクライアントが閉じるか時間が切れるまで開けておく
    tls.sock.shutdown(Shutdown::Write)?;
    tls.sock.set_read_timeout(Some(Duration::from_secs(2)))?;
    while tls.sock.read(&mut buf).is_ok_and(|n| n > 0) {}
    Ok(())
}
```

## B.2 bench/scripts/https_get.py (T-4)

```python
"""https-get: URL の本文を `urllib.request` で取り、状態、本文の長さ、本文の最後の行を出す。

python3 -I https_get.py CA_PEM URL
"""
import ssl
import sys
import urllib.request


def main():
    if len(sys.argv) != 3:
        print("usage: https_get CA_PEM URL", file=sys.stderr)
        sys.exit(2)
    ca, url = sys.argv[1], sys.argv[2]
    context = ssl.create_default_context(cafile=ca)
    with urllib.request.urlopen(url, context=context) as r:
        status = r.status
        body = r.read()
    print(status)
    print(len(body))
    # 本文は 16 バイトの行の列なので、最後の行は末尾の 16 バイトから改行を除いたものである
    print(body[-16:-1].decode())


main()
```

## B.3 bench/scripts.sh の `https_row` (T-4)

`numeric_row` の後に置く。`$data`、`$root`、`$runs`、`row`、`$https_server`、`server_pid` は `bench/scripts.sh` のもの。

```bash
# 行 `https-get`。`crates/eml_tls/examples/https_server.rs` を `$2` MiB の本文で裏で動かし、eml の `Http` と Python の
# `urllib.request` が `https://127.0.0.1:PORT/` から取る。比べも計測も同じ server につなぐ
https_row() {
  local label="$1" mib="$2"
  local port_file="$data/https-port.txt" port="" cpu_before cpu_after
  # 前の実行のポートを読まないよう、起こす前に消す
  rm -f "$port_file"
  "$https_server" "$mib" > "$port_file" &
  server_pid=$!
  # 起動で落ちた server を待ち続けない
  for _ in $(seq 100); do
    if [[ -s "$port_file" ]] || ! kill -0 "$server_pid" 2>/dev/null; then
      break
    fi
    sleep 0.1
  done
  port="$(head -n 1 "$port_file" 2>/dev/null || true)"
  if [[ ! "$port" =~ ^[0-9]+$ ]]; then
    echo "bench/scripts.sh: https_server がポートを出さなかった" >&2
    exit 1
  fi
  cpu_before="$(ps -o time= -p "$server_pid")"
  row "$label" "$mib MiB" "$root/bench/scripts/https_get.em" "$root/bench/scripts/https_get.py" /dev/null \
    "$root/crates/eml_tls/fixtures/ca.pem" "https://127.0.0.1:$port/"
  # server が eml の速さを抑えていないかを見るため、比べと計測の接続の分の CPU 時間を出す
  cpu_after="$(ps -o time= -p "$server_pid")"
  echo "bench/scripts.sh: https_server の CPU 時間 $cpu_before -> $cpu_after (接続 $((2 + 2 * runs)) 本)" >&2
  kill "$server_pid"
  wait "$server_pid" 2>/dev/null || true
  server_pid=""
}
```

## B.4 Linux で `https_row` を流す harness (T-4 の確かめだけに使い、コミットしない)

`bench/scripts.sh` は macOS でしか動かないので、コミットした `https_row` を `bench/scripts.sh` から切り出し、`row` だけを `compare.sh` と 3+3 回の実行に差し替えて流す。スクラッチの場所に置いて `bash harness.sh <リポジトリ>` で流す。

```bash
#!/usr/bin/env bash
# usage: harness.sh REPO   (環境変数: HTTPS_SERVER, EML, MIB)
set -euo pipefail
repo="$(cd "$1" && pwd)"
root="$repo"; runs=3
data="$(mktemp -d)"
eml="${EML:-$repo/target/release/eml}"
https_server="${HTTPS_SERVER:-$repo/target/release/examples/https_server}"
server_pid=""
trap 'if [[ -n "$server_pid" ]]; then kill "$server_pid" 2>/dev/null || true; fi; rm -rf "$data"' EXIT
row() {
  local label="$1" size="$2" eml_file="$3" py_file="$4" stdin="$5"; shift 5
  EML="$eml" "$repo/bench/scripts/compare.sh" --paths "$eml_file" "$py_file" "$@" < "$stdin"
  for _ in $(seq "$runs"); do
    s=$(date +%s.%N); "$eml" run "$eml_file" "$@" < "$stdin" > /dev/null; e=$(date +%s.%N); echo "eml $(echo "$e - $s" | bc)"
    s=$(date +%s.%N); python3 -I "$py_file" "$@" < "$stdin" > /dev/null; e=$(date +%s.%N); echo "py  $(echo "$e - $s" | bc)"
  done
  echo "| $label | $size |"
}
eval "$(sed -n '/^https_row() {/,/^}/p' "$repo/bench/scripts.sh")"
https_row https-get "${MIB:-64}"
echo "server_pid after: '$server_pid'"
```
