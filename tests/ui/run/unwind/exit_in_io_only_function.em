-- `<IO>` だけの row の関数は yield しない。3段目で `exit 4` を呼んでも、巻き戻しが各段の `File` を1回ずつ閉じる。
third : Unit -> <IO> Int
third () =
  let f = Fs.open "input.txt"
  let r = exit 4
  Fs.close f
  r

second : Unit -> <IO, Panic> Int
second () =
  let f = Fs.open "input.txt"
  let r = third () + 1
  Fs.close f
  r

first : Unit -> <IO, Panic> Int
first () =
  let f = Fs.open "input.txt"
  let r = second () + 1
  Fs.close f
  r

main : Unit -> <IO, Panic> Unit
main () =
  println "start"
  println (show (first ()))
