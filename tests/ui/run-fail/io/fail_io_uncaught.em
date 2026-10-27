-- `try_panic` は `fail_io` を捕まえず、呼んだ位置の実行時エラーになる (docs/spec/stdlib.md の「`fail_io`」)
main : Unit -> <IO, Panic> Unit
main () =
  println "before"
  match try_panic (fn () -> (fail_io Other "gave up" : Int)) with
    | Ok n -> println (show n)
    | Err m -> println "caught \{m}"
