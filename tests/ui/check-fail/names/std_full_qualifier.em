-- E1031: 標準ライブラリのモジュールを正式な名前 `Std.Fs` で修飾した。修飾子は1つのセグメントなので、短い名前 `Fs` を示す help を付ける (docs/spec/modules.md の「名前の解決」)。
main : Unit -> <IO> Unit
main () =
  let f = Std.Fs.open "a.txt"
  Std.Fs.close f
