-- `<IO>` だけの row の関数は yield しない。3段目で `exit 4` を呼んでも、巻き戻しが各段の `File` を1回ずつ閉じる。
import FileIn

third : Unit -> <IO> Int
third () =
  let f = FileIn.open "input.txt"
  let r = exit 4
  FileIn.close f
  r

second : Unit -> <IO, Panic> Int
second () =
  let f = FileIn.open "input.txt"
  let r = third () + 1
  FileIn.close f
  r

first : Unit -> <IO, Panic> Int
first () =
  let f = FileIn.open "input.txt"
  let r = second () + 1
  FileIn.close f
  r

main : Unit -> <IO, Panic> Unit
main () =
  println "start"
  println (show (first ()))
