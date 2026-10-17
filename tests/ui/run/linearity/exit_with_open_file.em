-- `File` を開いたまま `exit 1` を呼ぶ。`exit` の前の `drop` が `File` を閉じるので、検査モードで漏れがない。
main : Unit -> <IO> Unit
main () =
  let f = Fs.open "input.txt"
  println "before"
  exit 1
