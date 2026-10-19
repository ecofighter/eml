-- `try_io` は `Panic` を捕まえない。内側の `try_io` を通り抜けて、外の `try_panic` が捕まえる。
inner : Unit -> <IO, Panic> String
inner () =
  match try_io (fn () -> 1 / 0) with
    | Ok n -> "io ok \{n}"
    | Err e -> "io caught: \{e}"

main : Unit -> <IO> Unit
main () =
  println (show (try_panic inner))
