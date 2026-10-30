#!/bin/sh
# テスト用の CA と leaf を作り直す。流すたびに鍵が変わるので、出力はすべて一緒にコミットする。
# -not_before / -not_after が要るので OpenSSL 3.4 以上で流す:
#   nix shell nixpkgs#openssl -c crates/eml_tls/fixtures/gen.sh
set -eu
dir=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$dir/../../.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
# CA の鍵は残さない。leaf に署名したら捨てて、これ以上の証明書を作れないようにする
openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out "$tmp/ca.key"
# 期限を 100 年に取り、テストが時計に左右されないようにする
openssl req -x509 -new -key "$tmp/ca.key" -sha256 -subj "/CN=eml test CA" \
  -not_before 20260101000000Z -not_after 21260101000000Z \
  -addext "basicConstraints=critical,CA:TRUE" \
  -addext "keyUsage=critical,keyCertSign,cRLSign" \
  -out "$dir/ca.pem"
openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out "$dir/key.pem"
openssl req -new -key "$dir/key.pem" -subj "/CN=tls.localhost" -out "$tmp/leaf.csr"
# rustls は SAN だけを見るので、CN と別に 4 つの名前を SAN に入れる
cat > "$tmp/leaf.ext" <<EXT
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature
extendedKeyUsage=serverAuth
subjectAltName=DNS:tls.localhost,DNS:tls-cut.localhost,DNS:localhost,IP:127.0.0.1
EXT
openssl x509 -req -in "$tmp/leaf.csr" -CA "$dir/ca.pem" -CAkey "$tmp/ca.key" -sha256 \
  -not_before 20260101000000Z -not_after 21260101000000Z \
  -extfile "$tmp/leaf.ext" -out "$dir/cert.pem"
# UI テストはテストファイルのあるディレクトリを根にするので、カテゴリごとに CA を写す
for copy in "$root/tests/ui/run/tls" "$root/tests/ui/run-fail/tls" "$root/tests/ui/run/http" "$root/tests/ui/run-fail/http"; do
  mkdir -p "$copy"
  cp "$dir/ca.pem" "$copy/ca.pem"
done
