-- 実行時エラーの前に標準エラーに書いた行も、スナップショットに残る
main : Unit -> <IO> Unit
main () =
  eprintln "before"
  println (show (1 / 0))
