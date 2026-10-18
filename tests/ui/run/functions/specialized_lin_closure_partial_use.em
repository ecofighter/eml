-- `File` を捕まえた `once` の閉包を、片方の枝では呼び、もう片方の枝では高階関数に渡す。`unr` でない閉包は、使う所が
-- すべて書き換えられるときだけ書き換える。どちらの枝でも `File` は1回だけ閉じ、checked mode でもリークは出ない
call_once : (once Unit -> <IO> Unit) -> <IO> Unit
call_once g = g ()

close_in : Bool -> <IO> Unit
close_in direct =
  let f = Fs.open "input.txt"
  let c = fn () -> Fs.close f
  if direct then c () else call_once c

main : Unit -> <IO> Unit
main () =
  close_in True
  println "closed directly"
  close_in False
  println "closed through call_once"
