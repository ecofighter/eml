-- import を書かなくても、標準ライブラリの `Fs` を修飾子で呼べる (docs/spec/modules.md の「名前の解決」)。型の注釈の `Fs.File` も同じ。
read_text : Fs.File -> <IO> String
read_text f =
  let (f, text) = Fs.read_all f
  Fs.close f
  text

main : Unit -> <IO> Unit
main () = println (read_text (Fs.open "../files/input.txt"))
