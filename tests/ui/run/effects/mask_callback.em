-- コールバックの `get` は、`run` が自分で使う `State Int` の handler を飛ばし、外側の `State String` に届く。
effect State s where
  get : Unit -> s
  put : s -> Unit

run : (Unit -> <e> a) -> <State Int | e> a
run cb =
  let n = get ()
  cb ()

cb : Unit -> <State String> String
cb () = get () ++ "!"

main : Unit -> <IO> Unit
main () =
  let r = handle (handle run cb with
                    | get () k -> resume k 42
                    | put _ k -> resume k ()) with
            | get () k -> resume k "str"
            | put _ k -> resume k ()
  println r
