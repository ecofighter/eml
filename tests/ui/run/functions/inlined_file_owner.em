-- `File` を開いて閉じる小さな関数を2か所で呼ぶ。インライン化しても、それぞれの呼び出しが自分の `File` を1回だけ
-- 閉じ、検査モードのリークの検査を通る (docs/spec/core-ir.md の
-- 「インライン化」)。
with_file : String -> <IO> Int
with_file name =
  let f = Fs.open name
  let (text, f) = Fs.read_all f
  Fs.close f
  String.byte_length text

main : Unit -> <IO, Panic> Unit
main () =
  let a = with_file "input.txt"
  let b = with_file "input.txt"
  println (show (a + b))
