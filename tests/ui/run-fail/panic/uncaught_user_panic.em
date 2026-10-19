-- 捕まえない利用者の `panic` は、文言と位置を持つ実行時エラーになる。
check : Int -> <Panic> Int
check n = if n > 2 then panic "boom" else n

main : Unit -> <IO, Panic> Unit
main () =
  println "before"
  println (show (check 5))
