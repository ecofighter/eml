-- E2013: ローカル関数は呼ぶ前に本体を検査するので、型の決まらない引数への射影は誤りになる。
data Person = | Person { name : String }

main : Unit -> <IO> Unit
main () =
  let name_of p = p.name
  println (name_of (Person { name = "a" }))
