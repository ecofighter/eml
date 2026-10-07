-- `IO` の操作を `|>` で渡すと、実行時エラーは操作を包む関数 `op$Prelude.open` の名前で報告される (docs/implementation/status.md の「既知の制限」)。
main : Unit -> <IO> Unit
main () =
  let f = "missing.txt" |> open
  close f
