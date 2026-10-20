-- handle が EP の道に残る配列の変数の範囲を、外側の `try_panic` への中断と `exit` が抜ける。範囲の中の
-- `try_panic` が特殊化を止める。検査モードでも物体が残らない
discard : Array Int -> Int -> Int
discard _ n = n

inner : Int -> <Panic> Int
inner i =
  var a = MutArray.make 3 0
  match try_panic (fn () -> a[9]) with
    | Ok n -> a[0] := n
    | Err _ -> a[0] := 1
  let n = a[i]
  discard (MutArray.freeze a) n

main : Unit -> <IO, Panic> Unit
main () =
  match try_panic (fn () -> inner 5) with
    | Ok n -> println (show n)
    | Err message -> println message
  var b = MutArray.make 2 0
  match try_panic (fn () -> b[1]) with
    | Ok n -> println (show n)
    | Err message -> println message
  if b[0] == 0 then exit 4 else ()
  drop b
