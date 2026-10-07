-- import を書かないモジュールでは、ユーザーの `Fs` があっても、修飾子の `Fs` は標準ライブラリに届く。
-- main.em がユーザーの `Fs` を import していても、この規則は変わらない。
pub read_input : Unit -> <IO> String
read_input () =
  let (f, text) = Fs.read_all (Fs.open "../../files/input.txt")
  Fs.close f
  text
