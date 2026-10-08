-- extern の関数 `open` を `|>` で値として渡しても、実行時エラーは `Fs.open` を書いた位置で報告される。
main : Unit -> <IO> Unit
main () =
  let f = "missing.txt" |> Fs.open
  Fs.close f
