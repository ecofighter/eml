-- extern の関数 `open` を `|>` で渡すと、実行時エラーは extern の関数を包む関数 `extern$Std.Fs.open` の名前で報告される (docs/implementation/status.md の「既知の制限」)。
main : Unit -> <IO> Unit
main () =
  let f = "missing.txt" |> Fs.open
  Fs.close f
