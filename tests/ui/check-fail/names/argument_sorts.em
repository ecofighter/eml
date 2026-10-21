-- E1048: 型引数の種類は宣言の書き方で決まる。row の位置に型を書くか、型の位置に row を書くと誤りになる。row の
-- 位置の小文字の名前には、`<e>` に直す fix が付く。
data Iter <e> a =
  | Done
  | More a (once Unit -> <e> Iter <e> a)

lower : Iter e Int -> Int
lower _ = 0

typed : Iter Int Int -> Int
typed _ = 0

row_for_type : List <IO> -> Int
row_for_type _ = 0

main : Unit -> <IO> Unit
main () = println "unreachable"
