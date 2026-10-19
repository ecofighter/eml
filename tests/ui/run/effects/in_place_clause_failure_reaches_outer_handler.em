-- その場で動く `fn` 節が `fail` を呼ぶ。節の本体は handler の外側の `evv` で動くので、handle の本体の中の
-- `Fail.to_result` ではなく、handle の外の `Fail.to_result` が捕まえる。
import Fail (Fail(..))

effect Ask where
  ask : Unit -> Int

body : Unit -> <Ask, Panic> Result String Int
body () = Fail.to_result (fn () -> ask () + 1)

handled : Unit -> <Fail String, Panic> Result String Int
handled () =
  handle body () with
    | fn ask () -> Fail.fail "from the clause"

main : Unit -> <IO, Panic> Unit
main () =
  match Fail.to_result handled with
    | Ok (Ok n) -> println "ok \{n}"
    | Ok (Err e) -> println "inner caught: \{e}"
    | Err e -> println "outer caught: \{e}"
