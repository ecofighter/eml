-- `FileIn` を開いたまま `exit 1` を呼ぶ。`exit` の前の `drop` が `FileIn` を閉じるので、検査モードで漏れがない。
import FileIn

main : Unit -> <IO> Unit
main () =
  let f = FileIn.open "input.txt"
  println "before"
  exit 1
