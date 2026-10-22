-- `Lin` と宣言した型で、RC の葉を持たない構造体。`Ticket` は `Int` と `Float` の葉だけを持つ。構造体の値は葉の並びで
-- 渡り、一様な位置 (`Option` のフィールド) では箱に入る。`Lin` の箱は `decref` でなく `drop` で手放す
-- (docs/spec/lir.md の「Perceus」)。
import Float

data Ticket : Lin =
  | Ticket Int Float

issue : Int -> Ticket
issue n = Ticket n 1.5

bump : Int -> Ticket -> <Panic> Ticket
bump k t = if k == 0 then t else
  let Ticket n x = t
  bump (k - 1) (Ticket (n + 1) (x * 2.0))

redeem : Ticket -> <Panic> Float
redeem (Ticket n x) = Float.of_int n * x

main : Unit -> <IO, Panic> Unit
main () =
  println (show (redeem (bump 3 (issue 2))))
  match Some (issue 5) with
    | Some t -> println (show (redeem t))
    | None -> println "none"
