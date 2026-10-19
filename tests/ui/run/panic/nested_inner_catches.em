-- 入れ子の `try_panic` では内側が捕まえ、外側は内側の結果を `Ok` で返す。
inner : Unit -> <Panic> String
inner () =
  match try_panic (fn () -> 1 / 0) with
    | Ok n -> "inner ok \{n}"
    | Err e -> "inner caught: \{e}"

main : Unit -> <IO> Unit
main () =
  println (show (try_panic inner))
