-- 取り出した後に範囲の中で作った閉包を呼ぶと、その添字は空の配列に対して働き、範囲外の `Panic` になる
probe : Unit -> <Panic> (Array Int, Result String Int)
probe () =
  var a = MutArray.make 3 7
  let peek () = a[0]
  (MutArray.freeze a, try_panic peek)

main : Unit -> <IO, Panic> Unit
main () =
  let (a, r) = probe ()
  println (show (Array.to_list a))
  match r with
    | Ok n -> println (show n)
    | Err message -> println message
