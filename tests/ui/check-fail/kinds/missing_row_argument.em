-- E1015: row の型引数も省略できない。引数の数は、row の引数を含めて宣言と同じでなければならない。
data Iter <e> a =
  | Done
  | More a (once Unit -> <e> Iter <e> a)

count : Iter Int -> Int
count _ = 0

main : Unit -> <IO> Unit
main () = println "unreachable"
