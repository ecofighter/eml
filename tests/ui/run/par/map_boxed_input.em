-- `Par.map`の入力は、親が作った箱の要素を持つ。forkは配列とその要素を共有の印にするので、別のworkerの子が
-- それを読め、親はjoinの後で入力をもう一度使う (docs/spec/vm.mdの「`par`のforkとjoin」)。
import Std.Array
import Std.Par

upto : Int -> List Int -> <Panic> List Int
upto n acc = if n == 0 then acc else upto (n - 1) (n :: acc)

sum : List Int -> <Panic> Int
sum xs =
  match xs with
    | [] -> 0
    | x :: rest -> x + sum rest

main : Unit -> <IO, Panic> Unit
main () =
  let lists = Array.tabulate 3000 (fn i -> upto (i % 7) [])
  let sums = Par.map sum lists
  let longer = Par.mapi (fn i xs -> i :: xs) lists
  println (show (Par.reduce (fn a b -> a + b) 0 sums))
  println (show (Array.get longer 2999, Array.get lists 2999))
  println (show (sum (Array.get lists 6) + Array.length longer))
