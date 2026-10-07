-- 節の中の handle の本体で `k 1` を呼ぶと、再開した `act` の `log` はその handle を飛ばし、`main` の handler に届く。
effect Ask where
  ask : Unit -> Int

effect Log where
  log : String -> Unit

f : (Unit -> <Ask | e> Int) -> <e> Int
f action =
  handle action () with
    | ask () k ->
        handle k 1 with
          | log m k2 ->
              k2 ()

act : Unit -> <Ask, Log> Int
act () =
  let n = ask ()
  log "after"
  n + 1

main : Unit -> <IO> Unit
main () =
  let r = handle f act with
            | log m k ->
                println ("outer " ++ m)
                k ()
  println (show_int r)
