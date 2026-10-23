-- E3001: 自分を呼ぶローカル関数は何度も呼ばれうるので、`FileIn` を捕まえられない。2つ目の関数は `multi` の操作も行うが、
-- `FileIn` は操作の前に閉じるので E3006 は出ず、E3001 だけを報告する。
import FileIn

effect Choice where
  multi choose : Unit -> Bool

closes_in_loop : Unit -> <IO, Panic> Unit
closes_in_loop () =
  let f = FileIn.open "input.txt"
  let go n =
    FileIn.close f
    if n == 0 then () else go (n - 1)
  go 3

closes_across_multi : Unit -> <Choice, IO, Panic> Unit
closes_across_multi () =
  let f = FileIn.open "input.txt"
  let go n =
    FileIn.close f
    if n == 0 then (if choose () then () else ()) else go (n - 1)
  go 3

main : Unit -> <IO, Panic> Unit
main () = closes_in_loop ()
