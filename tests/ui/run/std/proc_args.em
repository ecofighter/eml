-- コマンドリテラルの引数の分け方を、子が受け取った引数で確かめる (docs/spec/expressions.md の「コマンドリテラル」)。
-- 引用の中の空白は区切らず、穴は引数の一部になり、`\{..xs}` は要素ごとに別の引数になる。子はこのファイルを
-- `child` の引数で流した eml で、受け取った引数を1行に1つ書く
import Env
import Proc
import Stdout

main : Unit -> <IO, Panic> Unit
main () =
  match Env.args () with
    | "child" :: args -> show_args args
    | _ -> parent ()

show_args : List String -> <IO> Unit
show_args args =
  match args with
    | [] -> ()
    | a :: rest ->
      println "[\{a}]"
      show_args rest

parent : Unit -> <IO, Panic> Unit
parent () =
  let dir = "out"
  let xs = ["1", "2"]
  let (out, code) = Proc.output `eml run proc_args.em child 'b c' --x=\{dir} \{..xs}`
  Stdout.close (Stdout.write out (Stdout.open ()))
  println "exit \{code}"
