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
