-- 実行時に選んだ `Int -> Int` の関数の値を、ループの中で10万回 `apply` する (S14e-1 の基準)。
-- 選ぶ関数が実行時まで決まらないので、既知の閉包への特殊化が効かない。S14e の規約は、`Int` の引数と結果を箱に包まずに
-- `apply` を通すので、`boxes` と `unboxes` が消える (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の「足す計測のプログラム」)
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
