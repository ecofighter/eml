-- `File` を捕まえる自分を参照しないローカル関数は、ラムダと同じ規則に従う。2回呼ぶ (E3002)、呼ばない (E3003)、
-- 片方の枝だけで呼ぶ (E3003)、`multi` の操作をまたいで持つ (E3006) はどれも誤りである。
import FileIn

effect Choice where
  multi choose : Unit -> Bool

called_twice : Unit -> <IO> Unit
called_twice () =
  let f = FileIn.open "input.txt"
  let finish () = FileIn.close f
  finish ()
  finish ()

never_called : Unit -> <IO> Unit
never_called () =
  let f = FileIn.open "input.txt"
  let finish () = FileIn.close f
  println "forgot"

called_on_one_branch : Bool -> <IO> Unit
called_on_one_branch b =
  let f = FileIn.open "input.txt"
  let finish () = FileIn.close f
  if b then finish () else println "kept"

held_across_multi : Unit -> <Choice, IO> Unit
held_across_multi () =
  let f = FileIn.open "input.txt"
  let finish () = FileIn.close f
  let b = choose ()
  finish ()
