-- 実行時に選んだ `Int -> Int` の関数の値を、ループの中で10万回 `apply` する。
-- 選ぶ関数が実行時まで決まらないので、既知の閉包への特殊化が効かない。関数の値の引数と結果は一様なので、`Int` は `apply` を通るたびに
-- `box` と `unbox` を通る。`Int` の箱はタグを付けるだけで、確保はない (docs/implementation/benchmarks.md の「呼び出しの規約の引き換え」)
import Env

inc : Int -> <Panic> Int
inc x = x + 1

dec : Int -> <Panic> Int
dec x = x - 1

pick : Unit -> <IO, Panic> (Int -> <Panic> Int)
pick () =
  match Env.args () with
    | Nil -> inc
    | _ -> dec

loop : (Int -> <Panic> Int) -> Int -> Int -> <Panic> Int
loop f n acc = if n == 0 then acc else loop f (n - 1) (f acc)

main : Unit -> <IO, Panic> Unit
main () = println (show (loop (pick ()) 100000 0))
