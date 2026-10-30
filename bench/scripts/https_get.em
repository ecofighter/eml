-- https-get: URL の本文を `Http` で取り、状態、本文の長さ、本文の最後の行を出す。`https_get CA_PEM URL`。
-- 比べる Python の実装は bench/scripts/https_get.py (docs/implementation/benchmarks.md の「スクリプトの計測」)
import Async
import Bytes
import Env
import Fs
import Http
import String

main : Unit -> <IO, Panic> Unit
main () =
  match Env.args () with
    | [ca_path, url] ->
      let options = { Http.defaults | ca = Fs.read_text ca_path }
      let r = Async.run (fn () -> Http.request "GET" url (Bytes.empty ()) options)
      let n = Bytes.length r.body
      println (show r.status)
      println (show n)
      -- 本文は 16 バイトの行の列なので、最後の行は末尾の 16 バイトから改行を除いたものである
      println (String.decode (Bytes.slice (n - 16) (n - 1) r.body))
    | _ ->
      eprintln "usage: https_get CA_PEM URL"
      exit 2
