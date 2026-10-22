-- E1031: import していない標準ライブラリのモジュールは修飾子にならない。短い名前 `String` には `import String` を、
-- 正式な名前 `Std.FileIn` には `import Std.FileIn` を足す help を付ける。同じ修飾子が何度出ても、それぞれに同じ help が付く
-- (docs/spec/modules.md の「名前の解決」)。
main : Unit -> <IO> Unit
main () =
  println (show (String.byte_length "ab"))
  Std.FileIn.close (Std.FileIn.open "a.txt")
