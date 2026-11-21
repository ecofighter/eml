-- 添字から計算した`Float`の要素を`Par.tabulate_unboxed`で作る。大きさは引数で受ける
-- (既定は8192で、塊は8つ)。回数のテストは2つの大きさで動かし、要素あたりの確保の数をその差から読む
-- (docs/implementation/benchmarks.mdの「回数のテスト」)。
import Std.Env
import Std.Float
import Std.Par
import Std.String
import Std.UnboxedArray

main : Unit -> <IO, Panic> Unit
main () =
  let n = size 8192
  let xs = Par.tabulate_unboxed n (fn i -> Float.of_int i * 0.5)
  println (Float.to_fixed 1 (UnboxedArray.get xs (n - 1)))

-- 引数がなければ既定の大きさで動く。
size : Int -> <IO, Panic> Int
size default =
  match Env.args () with
    | Nil -> default
    | [arg] ->
      match String.to_int arg with
        | Some n -> if n >= 1 then n else usage ()
        | None -> usage ()
    | _ -> usage ()

usage : Unit -> <Panic> Int
usage () = panic "usage: par_tabulate_float [SIZE]"
