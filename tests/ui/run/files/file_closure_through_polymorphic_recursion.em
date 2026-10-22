-- `File` を捕まえたクロージャ `shut_with file` を `g` に渡し、多相再帰で型変数 `a` を通して `k` を渡す。`f@[Int]` の
-- `k` は `unr` にならず、再帰の呼び出しは `f@[Int %m -> <IO, Panic> Int]` になる。`File` は、`n` が 0 になった所で `k` の
-- 鎖をたどって1回だけ閉じる (docs/spec/types.md の「instance での評価」)。
import Fs

f : a -> (a -> <IO, Panic> Int) -> (Int -> <IO> Int) -> Int -> <IO, Panic> Int
f x eat g n =
  if n == 0 then g (eat x)
  else
    let k = fn y -> g (eat x + y)
    f k (fn h -> h 1) (fn z -> z) (n - 1)

shut_with : Fs.File -> Int -> <IO> Int
shut_with file z =
  Fs.close file
  z

main : Unit -> <IO, Panic> Unit
main () =
  let file = Fs.open "input.txt"
  let n = f 1 (fn y -> y) (shut_with file) 3
  println (show n)
