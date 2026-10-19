-- その場で動く `fn` 節の中で桁あふれが起きる。節の本体は handler の外側の `evv` で動くので、handle の本体の中の
-- `try_panic` ではなく、handle の外の `try_panic` が捕まえる。
effect Tick where
  tick : Unit -> Unit

body : Unit -> <Tick, IO> Unit
body () =
  match try_panic (fn () -> tick ()) with
    | Ok () -> println "body ok"
    | Err e -> println "body caught: \{e}"

count : Unit -> <IO, Panic> Int
count () =
  handle body () from 9223372036854775807 with
    | fn tick () st -> ((), st + 1)
    | return _ st -> st

main : Unit -> <IO> Unit
main () =
  match try_panic count with
    | Ok n -> println "ok \{n}"
    | Err e -> println "outside caught: \{e}"
