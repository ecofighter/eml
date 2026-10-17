-- E3004: `once` の閉包は `Lin` の値なので、`_` で捨てられない。手放すには `drop` を書く。
data Job =
  | Job (once Unit -> Int)

ignore : Job -> Int
ignore job =
  match job with
    | Job _ -> 0

main : Unit -> <IO> Unit
main () = println (show (ignore (Job (fn () -> 1))))
