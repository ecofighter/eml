-- `++` は、ほかに持ち主がいない文字列だけをその場で伸ばす。あとで使う文字列、リテラル、両辺が同じ変数の場合は写すので、
-- あとで読む値は変わらない。
main : Unit -> <IO> Unit
main () =
  let a = "x" ++ "y"
  let b = a ++ "z"
  println a
  println b
  let c = b ++ b
  println c
  let d = "lit" ++ "!"
  println "lit"
  println d
