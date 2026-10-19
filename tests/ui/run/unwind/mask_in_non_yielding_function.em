-- `Fail` だけの row の `run` は yield しない。`mask` 付きの呼び出しの中の `fail` は、内側の handler を飛ばして
-- 外側の handler へ巻き戻す。
effect Fail where
  never fail : String -> a

run : (Unit -> <e> Int) -> <Fail, Panic | e> Int
run cb =
  let x = cb ()
  if x == 0 then fail "inner" else x + 1

main : Unit -> <IO, Panic> Unit
main () =
  let r = handle (handle run (fn () -> fail "outer") with
                    | ctl fail s -> "inner handler caught \{s}"
                    | return x -> "returned \{x}") with
            | ctl fail s -> "outer handler caught \{s}"
  println r
