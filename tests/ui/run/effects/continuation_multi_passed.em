-- `multi` の操作の継続を関数に渡し、渡した先で2回呼ぶ。
effect Choose where
  multi choose : Unit -> Bool

both : (Bool -> <e> String) -> <e> String
both f = f True ++ "," ++ f False

main : Unit -> <IO> Unit
main () =
  let r =
    handle show_int (if choose () then 10 else 20) with
      | choose () k -> both k
  println r
