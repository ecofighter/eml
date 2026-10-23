-- 出力を取る起動 (docs/spec/stdlib.md の「`Proc`」)。`output` は終了コードを値で返し、0 でなくても失敗しない。
-- `capture` は 0 で終わった子の標準出力を返す。標準入力は空なので、`cat` はすぐに終わる
import Proc

main : Unit -> <IO, Panic> Unit
main () =
  println (show (Proc.capture `echo hi`))
  println (show (Proc.output `sh -c 'echo partial; exit 5'`))
  println (show (Proc.capture `cat`))
  let (bytes, code) = Proc.output_bytes `printf 'a\\377'`
  println "\{show bytes} \{code}"
  println (show (Proc.capture_bytes `printf ab`))
  let cmd = `echo` |> Proc.arg "one" |> Proc.args ["two", "three"]
  println (display cmd)
  println (show (Proc.capture cmd))
