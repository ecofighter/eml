-- `import Fs` は根のユーザーの `Fs.em` を読み、`import Std.Fs as F` は標準ライブラリの `Fs` を読む。どちらも同時に使える。
import Fs
import Std.Fs as F
import Other

main : Unit -> <IO> Unit
main () =
  println (Fs.describe ())
  let f = F.open "../../files/input.txt"
  let (f, text) = F.read_all f
  F.close f
  println text
  println (Other.read_input ())
