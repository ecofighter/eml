-- 別のモジュールで宣言した row の型引数を持つ `Iter` を、import した名前でフィールドに、修飾した名前
-- `Gen.Iter <IO, Panic> Int` でシグネチャに書く (docs/spec/types.md の「row の型引数」)。
import Gen (Iter(..), Yield(..), generate)

data Source <e> =
  | Source { name : String, items : Iter <e> Int }

numbers : Int -> Int -> <Yield Int, IO, Panic> Unit
numbers i n =
  if i > n then ()
  else
    println "yield \{i}"
    yield i
    numbers (i + 1) n

sum : Gen.Iter <IO, Panic> Int -> <IO, Panic> Int
sum it =
  match it with
    | Gen.Done -> 0
    | Gen.More x next -> x + sum (next ())

report : Source <IO, Panic> -> <IO, Panic> Unit
report source =
  let Source { name, items } = source
  let total = sum items
  println "\{name}: \{total}"

main : Unit -> <IO, Panic> Unit
main () =
  report (Source { name = "numbers", items = generate (fn () -> numbers 1 3) })
