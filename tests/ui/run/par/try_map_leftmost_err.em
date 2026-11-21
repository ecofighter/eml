-- `Par.try_map`はすべての要素に関数を呼んだ後で、最も左の`Err`を返す。後ろの塊も失敗する場合も同じである。
-- `Err`がなければ写した配列を返す (docs/spec/stdlib.mdの「`Std.Par`」)。
import Std.Array
import Std.Par

check : Int -> <Panic> Result String Int
check x = if x % 1500 == 1499 then Err "bad \{show x}" else Ok (x * 2)

count : Array (Result String Int) -> Int -> Int -> <Panic> Int
count rs i acc =
  if i == Array.length rs then
    acc
  else
    match Array.get rs i with
      | Err _ -> count rs (i + 1) (acc + 1)
      | Ok _ -> count rs (i + 1) acc

main : Unit -> <IO, Panic> Unit
main () =
  let xs = Par.tabulate 5000 (fn i -> i)
  match Par.try_map check xs with
    | Ok _ -> println "unreachable"
    | Err e -> println e
  println (show (count (Par.map check xs) 0 0))
  match Par.try_map (fn x -> if x < 0 then Err x else Ok (x + 1)) xs with
    | Ok ys -> println (show (Array.length ys, Array.get ys 0, Array.get ys 4999))
    | Err e -> println "unreachable \{show e}"
  match Par.try_map check (Par.tabulate 1000 (fn i -> i)) with
    | Ok ys -> println (show (Array.length ys))
    | Err e -> println e
  match Par.try_map check (Array.from_list ([] : List Int)) with
    | Ok ys -> println (show (Array.length ys))
    | Err e -> println e
  let late x = if x == 2500 then Err "err \{show x}" else if x == 3500 then panic "panic \{show x}" else Ok x
  println (show (try_panic (fn () -> match Par.try_map late xs with
    | Ok _ -> "ok"
    | Err e -> e)))
  let early x = if x == 1100 then Err "err \{show x}" else if x == 3500 then panic "panic \{show x}" else Ok x
  println (show (try_panic (fn () -> match Par.try_map early xs with
    | Ok _ -> "ok"
    | Err e -> e)))
