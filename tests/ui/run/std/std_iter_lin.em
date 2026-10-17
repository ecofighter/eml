-- `Iter` の変換は `Lin` の要素にも使える (docs/spec/stdlib.md)。`zip` は右が先に終わると、左から引いた要素と左の残りを
-- 捨てる。左が先に終わると右の残りを捨てる。どちらも checked mode の漏れの検査で確かめる
import Iter (Iter)

data Ticket : Lin =
  | Ticket Int

spend : Ticket -> Int
spend (Ticket n) = n

tickets : List Int -> Iter <e> Ticket
tickets ns = Iter.map (fn n -> Ticket n) (Iter.from_list ns)

show_zipped : Iter <IO> (Ticket, Ticket) -> <IO> Unit
show_zipped it = Iter.each (fn (a, b) -> println "\{spend a} \{spend b}") it

main : Unit -> <IO> Unit
main () =
  show_zipped (Iter.zip (tickets [1, 2, 3]) (tickets [4]))
  println "right ended"
  show_zipped (Iter.zip (tickets [5]) (tickets [6, 7, 8]))
  println "left ended"
  Iter.each (fn (i, t) -> println "\{i} \{spend t}") (Iter.enumerate 0 (Iter.take 2 (tickets [9, 10, 11])))
