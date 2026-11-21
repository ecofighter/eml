-- どの塊も最後まで、または自分の`Panic`まで動き、添字がいちばん小さい塊の`Panic`が起こし直される。
-- `par`は左の子の`Panic`を先に起こし、左の子は分割のどちらの段でも小さい添字を持つためである。
-- 空の範囲は何も呼ばない (docs/spec/stdlib.mdの「`Std.Par`」)
import Std.Par

main : Unit -> <IO, Panic> Unit
main () =
  println (show (try_panic (fn () -> Par.for_range 0 5000 (fn i -> if i == 4000 || i == 2500 then panic "at \{show i}" else ()))))
  println (show (try_panic (fn () -> Par.reduce_range (fn a b -> a + b) 0 0 5000 (fn i -> 10 / (i - 3000)))))
  println (show (try_panic (fn () -> Par.reduce_range_grain 10 (fn a b -> if a + b > 100 then panic "join \{show a} \{show b}" else a + b) 0 0 40 (fn i -> i))))
  Par.for_range 0 3000 (fn i -> if i < 0 then panic "negative" else ())
  Par.for_range_grain 100 0 1000 (fn _ -> ())
  Par.for_range 5 5 (fn _ -> panic "empty")
  println (show (Par.reduce_range (fn a b -> a + b) 7 5 5 (fn i -> i)))
  println (show (Par.reduce_range (fn a b -> a + b) 7 0 5000 (fn i -> i)))
  println (show (try_panic (fn () -> Par.for_range_grain 0 0 5 (fn _ -> ()))))
  Par.for_range_grain 0 5 5 (fn _ -> panic "empty")
  println (show (try_panic (fn () -> Par.for_range 0 5000 (fn i -> if i == 1500 || i == 100 then panic "at \{show i}" else ()))))
  println (show (try_panic (fn () -> Par.for_range 0 5000 (fn i -> if i == 4500 || i == 3100 then panic "at \{show i}" else ()))))
  println (show (try_panic (fn () -> Par.for_range 0 5000 (fn i -> if i == 2100 || i == 1500 then panic "at \{show i}" else ()))))
  println (show (try_panic (fn () -> Par.for_range (-9223372036854775807) 9223372036854775807 (fn _ -> panic "called"))))
  Par.for_range 9 3 (fn _ -> panic "empty")
  println (show (Par.reduce_range (fn a b -> a + b) 7 9 3 (fn i -> i)))
