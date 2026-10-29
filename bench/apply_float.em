-- 実行時に選んだ `Float -> Float` の関数の値を、ループの中で10万回 `apply` する。
-- `apply_int` の `Float` にしたもので、`Float` の引数と結果の箱が `apply` を通る費用を見る。関数の値の引数と結果は一様なので、呼ぶたびに
-- 引数と結果の箱を1つずつ確保する (docs/implementation/benchmarks.md の「呼び出しの規約の引き換え」)
import Env

half : Float -> <Panic> Float
half x = x * 0.5 + 1.0

twice : Float -> <Panic> Float
twice x = x * 2.0 - 1.0

pick : Unit -> <IO, Panic> (Float -> <Panic> Float)
pick () =
  match Env.args () with
    | Nil -> half
    | _ -> twice

loop : (Float -> <Panic> Float) -> Int -> Float -> <Panic> Float
loop f n acc = if n == 0 then acc else loop f (n - 1) (f acc)

main : Unit -> <IO, Panic> Unit
main () = println (show (loop (pick ()) 100000 0.0))
