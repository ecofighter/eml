-- その場で動く `fn` 節が、自分の `try_io` で IO の失敗を捕まえ、代わりの値で再開する。
import Fs

effect Ask where
  ask : Unit -> Int

body : Unit -> <Ask, Panic> Int
body () = ask () + 1

handled : Unit -> <IO, Panic> Int
handled () =
  handle body () with
    | fn ask () ->
      match try_io (fn () -> Fs.read_text "missing.txt") with
        | Ok _ -> 0
        | Err e ->
          println "clause caught: \{e}"
          7

main : Unit -> <IO, Panic> Unit
main () = println "got \{handled ()}"
