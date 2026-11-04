-- `.|` は段の標準出力を次の段の標準入力に pipe でつなぎ、全部の段を1回で起動する (docs/spec/stdlib.md の「`Proc`」)。
-- 段はこのファイルを引数で流した eml である。最後の段が先に終わっても、前の段は書き込みの誤りで終わり、`capture` は
-- 待ち終える。その段の終了コードと標準エラーは、いつ書き込みが断られるかで変わるので、表示しない
import Env
import Proc ((.|))
import Stdin
import String

main : Unit -> <IO, Panic> Unit
main () =
  match Env.args () with
    | ["produce"] -> produce 16384
    | ["copy"] -> copy (Stdin.open ()) ""
    | ["copy", prefix] -> copy (Stdin.open ()) prefix
    | ["first"] -> first ()
    | ["exit3"] ->
      println "one"
      exit 3
    | _ -> parent ()

-- 1行は64バイトで、16384 行で 1 MiB になる
produce : Int -> <IO, Panic> Unit
produce n =
  if n == 0 then ()
  else
    println "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcde"
    produce (n - 1)

copy : Stdin.Stdin -> String -> <IO, Panic> Unit
copy h prefix =
  match Stdin.read_line h with
    | (Some line, h) ->
      println (prefix ++ line)
      copy h prefix
    | (None, h) -> Stdin.close h

first : Unit -> <IO, Panic> Unit
first () =
  match Stdin.read_line (Stdin.open ()) with
    | (Some line, h) ->
      println "first: \{line}"
      Stdin.close h
    | (None, h) -> Stdin.close h

stage : String -> Cmd
stage mode = `eml run proc_pipeline.em \{mode}`

parent : Unit -> <IO, Panic> Unit
parent () =
  let produce = Proc.discard_stderr (stage "produce")
  let copy = Proc.discard_stderr (stage "copy")
  println (String.trim (Proc.capture (produce .| copy .| stage "first")))
  let exits = stage "exit3" .| stage "copy"
  let (out, child) = Proc.read_all (Proc.spawn exits)
  println (String.trim out)
  println "wait_all \{show (Proc.wait_all child)}"
  println "run \{Proc.run exits}"
  let prefixed = `eml run proc_pipeline.em copy b:`
  let whole = Proc.capture (Proc.stdin_text "x" (stage "copy" .| prefixed))
  let head = Proc.capture (Proc.stdin_text "x" (stage "copy") .| prefixed)
  println (String.trim whole)
  println "same \{show (whole == head)}"
