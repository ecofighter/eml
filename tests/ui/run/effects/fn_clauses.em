-- `fn` の節は `k` を書かず、本体の値で再開する (docs/spec/expressions.md の「handler」)。状態のない handler の
-- `fn` の節、`multi` の操作の `fn` の節、別のエフェクトを起こす本体を持つ `fn` の節、`fn` と `ctl` の節を混ぜた
-- handler、`fn` の節の本体の中で入れ子にした `fn` の節を、それぞれ動かす。
effect Ask where
  ask : Unit -> Int

effect Choice where
  multi choose : Unit -> Bool

effect Log where
  log : String -> Unit

effect Input where
  next : Unit -> Int
  skip : Unit -> Int
  never stop : String -> a

with_ask : Int -> (Unit -> <Ask | e> a) -> <e> a
with_ask n action =
  handle action () with
    | fn ask () -> n

asked : Unit -> Int
asked () = with_ask 20 (fn () -> ask () + ask () + 2)

all_true : Unit -> Int
all_true () =
  handle (if choose () then 1 else 0) + (if choose () then 10 else 0) with
    | fn choose () -> True

logged : Unit -> <Log> Int
logged () =
  handle ask () * 2 with
    | fn ask () ->
        log "asked once"
        21

divide : Int -> <Input> Int
divide d =
  if d == 0 then stop "division by zero"
  else if d < 0 then skip ()
  else next () / d

attempt : Int -> String
attempt d =
  handle divide d with
    | fn next () -> 84
    | ctl skip () k -> "skipped, then " ++ k 0
    | ctl stop message -> "failed: " ++ message
    | return x -> "ok: " ++ show x

nested : Unit -> Int
nested () =
  handle ask () + 1 with
    | fn ask () ->
        handle ask () * 10 with
          | fn ask () -> 4

main : Unit -> <IO> Unit
main () =
  println (show (asked ()))
  println (show (all_true ()))
  let n =
    handle logged () with
      | fn log message -> println message
  println (show n)
  println (attempt 2)
  println (attempt 0)
  println (attempt (0 - 1))
  println (show (nested ()))
