-- `Proc.capture` の誤りは、起動、読み、終了コードの順に出る (docs/spec/stdlib.md の「`Proc`」)。UTF-8 でない出力を
-- 出して 0 でなく終わった子は、読みの誤りで止まる。子はこのファイルを `child` の引数で流した eml である
import Env
import Proc
import Stdout
import Bytes
import Byte

main : Unit -> <IO, Panic> Unit
main () =
  match Env.args () with
    | ["child"] ->
      Stdout.close (Stdout.write_bytes (Bytes.push (Byte.of_int 255) (Bytes.empty ())) (Stdout.open ()))
      exit 1
    | _ ->
      println "before"
      println (Proc.capture `eml run proc_capture_not_utf8.em child`)
