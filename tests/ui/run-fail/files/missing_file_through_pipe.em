-- extern の関数 `open` を `|>` で値として渡しても、実行時エラーは `FileIn.open` を書いた位置で報告される。
import FileIn

main : Unit -> <IO> Unit
main () =
  let f = "missing.txt" |> FileIn.open
  FileIn.close f
