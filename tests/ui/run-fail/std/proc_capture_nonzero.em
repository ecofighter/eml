-- `try_io` で捕まえない `Proc.capture` の 0 でない終了は、実行時エラーになる (docs/spec/stdlib.md の「実行時エラー」)。
-- 子はこのファイルを `child` の引数で流した eml である
import Env
import Proc

main : Unit -> <IO, Panic> Unit
main () =
  match Env.args () with
    | ["child"] ->
      println "partial"
      exit 3
    | _ ->
      println "before"
      println (Proc.capture `eml run proc_capture_nonzero.em child`)
