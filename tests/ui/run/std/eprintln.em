-- `eprintln` は標準エラーに書くので、標準出力には何も出ない
main : Unit -> <IO> Unit
main () =
  eprintln "to the standard error"
  println "to the standard output"
