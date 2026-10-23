-- windows: skip (子のプロセスに POSIX のツールを使う)
-- `Proc.env` は環境変数を1つ上書きし、同じ名前を2回設定したら後の値が勝つ。空の名前と `=` を含む名前は起動の誤りで
-- ある (docs/spec/stdlib.md の「`Proc`」)
import Proc

main : Unit -> <IO, Panic> Unit
main () =
  let c = `sh -c 'echo "[$FOO] [$BAR]"'`
  println (show (Proc.capture (Proc.env "FOO" "foo" c)))
  println (show (c |> Proc.env "FOO" "a" |> Proc.env "BAR" "b c" |> Proc.env "FOO" "z" |> Proc.capture))
  println (show (Proc.capture c))
  println (show_result (try_io (fn () -> Proc.capture (Proc.env "A=B" "x" c))))
  println (show_result (try_io (fn () -> Proc.capture (Proc.env "" "x" c))))

show_result : Result IOError String -> String
show_result (Ok out) = "ok \{show out}"
show_result (Err e) = "\{show e.kind}: \{e}"
