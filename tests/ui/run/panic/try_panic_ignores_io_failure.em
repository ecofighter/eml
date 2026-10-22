-- `try_panic` は IO の失敗を捕まえない。内側の `try_panic` を通り抜けて、外の `try_io` が捕まえる。
import FileIn

inner : Unit -> <IO> String
inner () =
  match try_panic (fn () -> FileIn.close (FileIn.open "missing.txt")) with
    | Ok () -> "panic ok"
    | Err e -> "panic caught: \{e}"

main : Unit -> <IO> Unit
main () =
  match try_io inner with
    | Ok text -> println "io ok, \{text}"
    | Err e -> println "io caught: \{e}"
