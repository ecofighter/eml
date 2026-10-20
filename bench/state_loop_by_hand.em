-- `state_loop.em` のループを、状態を引数で手で受け渡す形に書いた版。`run_by_hand` と `exec_by_hand` は、
-- `Std.State` の `run` と `exec` をなぞる
bump : Int -> Int -> <Panic> Int
bump i st = st + i

loop : Int -> Int -> Int -> <Panic> Int
loop i n st = if i > n then st else loop (i + 1) n (bump i st)

run_by_hand : s -> (s -> <e> s) -> <e> (Unit, s)
run_by_hand init action = ((), action init)

exec_by_hand : s -> (s -> <e> s) -> <e> s
exec_by_hand init action =
  let (_, st) = run_by_hand init action
  st

main : Unit -> <IO, Panic> Unit
main () = println (show (exec_by_hand 0 (fn st -> loop 1 100000 st)))
