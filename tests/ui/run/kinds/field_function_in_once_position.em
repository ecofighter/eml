-- 射影 `r.cb` で取り出した `->` のフィールドの関数は、読む式で広がるので `once` の位置に渡せる
-- (docs/spec/types.md の「値を読む式の矢印の線形性」)。
data R =
  | R { cb : Unit -> String }

run_once : (once Unit -> String) -> String
run_once f = f ()

hello : Unit -> String
hello () = "hello"

get : R -> (Unit -> String)
get r = r.cb

main : Unit -> <IO> Unit
main () =
  let r = R { cb = hello }
  println (run_once (get r))
  println (run_once r.cb)
