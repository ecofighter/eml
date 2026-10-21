-- E0011: `once` の後ろは関数型でなければならない。`once Int` は構文の誤りになる。
data Job =
  | Job (once Int)

main : Unit -> <IO> Unit
main () = println "unreachable"
