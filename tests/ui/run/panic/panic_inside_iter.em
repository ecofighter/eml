-- `Iter.generate` を `Iter.fold` で引き、畳み込みの関数の中でゼロ除算が起きる。外の `try_panic` が捕まえ、
-- 生産者の継続と `FileIn` は巻き戻しで手放す。検査モードの漏れの検査も通る。`Iter.fold` だけの形は融合で継続を作らない
-- ループになり、`Iter.filter` を重ねた形は生産者の `once` の継続を捕まえたまま中断する。
import Iter
import Yield (Yield(..))
import FileIn
import Fs

numbers : Int -> Unit -> <Yield Int, IO, Panic> Unit
numbers n () =
  let go file i =
    if i > n then FileIn.close file
    else
      yield i
      go file (i + 1)
  go (FileIn.open "numbers.txt") 1

total : Int -> Unit -> <IO, Panic> Int
total d () = Iter.fold (fn a b -> a + b / (d - b)) 0 (Iter.generate (numbers 5))

filtered : Int -> Unit -> <IO, Panic> Int
filtered d () =
  Iter.fold (fn a b -> a + b / (d - b)) 0 (Iter.filter (fn x -> x > 1) (Iter.generate (numbers 5)))

main : Unit -> <IO> Unit
main () =
  Fs.write_text "numbers.txt" "x\n"
  println (show (try_panic (total 3)))
  println (show (try_panic (total 9)))
  println (show (try_panic (filtered 3)))
  println (show (try_panic (filtered 9)))
