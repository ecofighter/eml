-- その場で動く `fn` 節が無いファイルを開く。節の本体は handler の外側の `evv` で動くので、handle の本体の中の
-- `try_io` ではなく、handle の外の `try_io` が IO の失敗を捕まえる。
import FileIn

effect Ask where
  ask : Unit -> Int

body : Unit -> <Ask, IO, Panic> String
body () =
  match try_io (fn () -> ask () + 1) with
    | Ok n -> "body ok: \{n}"
    | Err e -> "body caught: \{e}"

handled : Unit -> <IO, Panic> String
handled () =
  handle body () with
    | fn ask () ->
      FileIn.close (FileIn.open "missing.txt")
      1

main : Unit -> <IO, Panic> Unit
main () =
  match try_io handled with
    | Ok text -> println text
    | Err e -> println "outside caught: \{e}"
