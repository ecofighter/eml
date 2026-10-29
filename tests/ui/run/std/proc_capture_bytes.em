-- 出力をバイト列で取る起動 (docs/spec/stdlib.md の「`Proc`」)。UTF-8 を確かめない。子はこのファイルを `child` の
-- 引数で流した eml である
import Env
import Proc
import Stdout
import Bytes
import Byte

main : Unit -> <IO, Panic> Unit
main () =
  match Env.args () with
    | ["child", "bytes"] -> Stdout.close (Stdout.write_bytes (Bytes.push (Byte.of_int 255) (Bytes.of_string "a")) (Stdout.open ()))
    | ["child", "ab"] -> Stdout.close (Stdout.write "ab" (Stdout.open ()))
    | _ -> parent ()

parent : Unit -> <IO, Panic> Unit
parent () =
  let child = `eml run proc_capture_bytes.em child`
  let (bytes, code) = Proc.output_bytes (Proc.arg "bytes" child)
  println "\{show bytes} \{code}"
  println (show (Proc.capture_bytes (Proc.arg "ab" child)))
