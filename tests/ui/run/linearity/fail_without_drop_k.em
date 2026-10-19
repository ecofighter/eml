-- 節の `k` を呼ばずに `fail` する枝は、`drop k` を書かなくてよい。`k` は `fail` の前に手放す。
import Fail (Fail(..))
import Yield (Yield(..))

numbers : List Int -> Unit -> <Yield Int | e> Unit
numbers xs () =
  match xs with
    | [] -> ()
    | x :: rest ->
      Yield.yield x
      numbers rest ()

-- 3より大きい値を受け取ると失敗し、そうでなければ値の数を返す
count_small : (Unit -> <Yield Int, Fail String | e> Unit) -> <Fail String, Panic | e> Int
count_small body =
  handle body () from 0 with
    | ctl yield n k count ->
      if n > 3 then Fail.fail "too large: \{n}"
      else k () (count + 1)
    | return _ count -> count

main : Unit -> <IO, Panic> Unit
main () =
  println (show (Fail.to_result (fn () -> count_small (numbers [1, 2, 3]))))
  println (show (Fail.to_result (fn () -> count_small (numbers [1, 5, 2]))))
