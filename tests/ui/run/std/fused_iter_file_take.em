-- `File` を持つ生産者を `Iter.generate` で包み、`Iter.fold` で引く。`Iter.fold` だけの形と `Iter.map` を重ねた形は、
-- 融合で `File` を引数に回すループになる (`crates/eml_core_ir/tests/optimize.rs` の
-- `a_fold_over_a_file_producer_is_fused`)。`Iter.filter` と `Iter.take` を重ねた形は、一部だけが融合する。生産者は
-- 最後まで引くと `File` を閉じる。`Iter.take` が途中でやめると、残りの継続ごと `File` を捨てて閉じる。どの形でも
-- `File` はちょうど1回閉じ、閉じなければ checked mode の漏れの検査で落ちる
import Iter
import Yield (Yield(..))

numbers : Int -> Unit -> <Yield Int, IO> Unit
numbers n () =
  let go file i =
    if i > n then Fs.close file
    else
      yield i
      go file (i + 1)
  go (Fs.open "numbers.txt") 1

main : Unit -> <IO> Unit
main () =
  Fs.write_text "numbers.txt" "x\n"
  println (show (Iter.fold (fn a b -> a + b) 0 (Iter.generate (numbers 5))))
  println (show (Iter.fold (fn a b -> a + b) 0 (Iter.take 2 (Iter.generate (numbers 5)))))
  println (show (Iter.fold (fn a b -> a + b) 0 (Iter.map (fn x -> x * 2) (Iter.generate (numbers 5)))))
  println (show (Iter.fold (fn a b -> a + b) 0 (Iter.filter (fn x -> x > 2) (Iter.generate (numbers 5)))))
  println (show (Iter.fold (fn a b -> a + b) 0 (Iter.take 3 (Iter.filter (fn x -> x > 1) (Iter.generate (numbers 5))))))
