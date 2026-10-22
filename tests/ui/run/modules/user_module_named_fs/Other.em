-- `import Std.Fs` は、根にユーザーの `Fs.em` があっても標準ライブラリの `Fs` を指す。
-- main.em がユーザーの `Fs` を import していても、この規則は変わらない。
import Std.Fs

pub read_input : Unit -> <IO> String
read_input () = Fs.read_text "../../files/input.txt"
