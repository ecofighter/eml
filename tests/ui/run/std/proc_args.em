-- windows: skip (子のプロセスに POSIX のツールを使う)
-- コマンドリテラルの引数の分け方を、子が受け取った引数で確かめる (docs/spec/expressions.md の「コマンドリテラル」)。
-- 引用の中の空白は区切らず、穴は引数の一部になり、`\{..xs}` は要素ごとに別の引数になる
import Proc
import Stdout

main : Unit -> <IO, Panic> Unit
main () =
  let dir = "out"
  let xs = ["1", "2"]
  let (out, code) = Proc.output `sh -c 'for a in "$@"; do echo "[$a]"; done' sh 'b c' --x=\{dir} \{..xs}`
  Stdout.close (Stdout.write out (Stdout.open ()))
  println "exit \{code}"
