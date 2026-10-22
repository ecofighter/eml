-- E1031: 標準ライブラリのモジュールを正式な名前 `Std.FileIn` で修飾した。修飾子は1つのセグメントなので、`import Std.FileIn` を足して短い名前 `FileIn` で書く help を付ける (docs/spec/modules.md の「名前の解決」)。
main : Unit -> <IO> Unit
main () =
  let f = Std.FileIn.open "a.txt"
  Std.FileIn.close f
