-- `try_panic` が整数の桁あふれを捕まえ、今の実行時エラーの文言を `Err` で返す。
main : Unit -> <IO> Unit
main () =
  let r = try_panic (fn () -> 9223372036854775807 + 1)
  println (show r)
