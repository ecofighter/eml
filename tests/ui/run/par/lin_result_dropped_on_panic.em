-- 一方の子が`Panic`を起こすと、`par`は起こす前にもう一方の子の結果を捨てる。結果は`Lin`の`MutArray`とリストである。
-- 捨てなければ検査モードが漏れを報告する (docs/spec/stdlib.mdの「`Std.Par`」)。
import Std.Par
import Std.MutArray
import Std.Array

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

upto : Int -> List Int -> <Panic> List Int
upto n acc = if n == 0 then acc else upto (n - 1) (n :: acc)

size : Result String (MutArray Int, MutArray Int) -> <Panic> String
size r =
  match r with
    | Ok (a, b) -> show (Array.length (MutArray.freeze a) + Array.length (MutArray.freeze b))
    | Err m -> m

main : Unit -> <IO, Panic> Unit
main () =
  println (size (try_panic (fn () -> Par.par (fn () -> MutArray.make (spin 40 + 3) 0) (fn () -> MutArray.make 4 0))))
  println (size (try_panic (fn () -> Par.par (fn () -> MutArray.make (spin 40 + 3) 0) (fn () -> panic "right"))))
  println (size (try_panic (fn () -> Par.par (fn () -> MutArray.make (spin 40 + 1 / 0) 0) (fn () -> MutArray.make 4 0))))
  let r = try_panic (fn () -> Par.par (fn () -> spin 40 + panic "list") (fn () -> upto 3 []))
  println (show r)
