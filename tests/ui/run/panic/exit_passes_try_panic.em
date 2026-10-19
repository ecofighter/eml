-- `try_panic` は `exit` を捕まえない。巻き戻しは `try_panic` を通り抜けて `Finish` まで届く。
main : Unit -> <IO> Unit
main () =
  println "before"
  match try_panic (fn () -> exit 3) with
    | Ok () -> println "never ok"
    | Err e -> println "never caught: \{e}"
