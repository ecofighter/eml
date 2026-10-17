-- E3006: a clause that ends in `fail` drops `k` just before the call, so it holds `k` across `choose`.
effect Choose where
  multi choose : Unit -> Bool

effect Ask where
  once ask : Unit -> Int

effect Fail where
  never fail : String -> a

run : Unit -> <Choose, Fail> Int
run () =
  handle ask () with
    | ctl ask () k ->
        let b = choose ()
        fail "x"

main : Unit -> <IO> Unit
main () = ()
