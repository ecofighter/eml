-- E2020: 上限 `never` を書いた引数に `IO` を起こす関数を渡している。
helper : (Unit -> <never e> Int) -> <e> Int
helper f = f ()

noisy : Unit -> <IO> Int
noisy () =
  println "x"
  1

main : Unit -> <IO> Unit
main () = println (show (helper noisy))
