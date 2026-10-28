-- `try_send`、`try_recv`、`close` は `Async` を持たないので、`Async.run` の外でも使える。channel の品物には channel も
-- 置ける。負の容量は 0 として扱う。閉じて空になって放した channel の値は、同じ添字を新しい channel が使っても、
-- 閉じて空の channel として振る舞う (docs/spec/stdlib.md の「`Std.Chan`」)。
import Chan

main : Unit -> <IO> Unit
main () =
  let c = Chan.new 1
  println (show (Chan.try_recv c))
  println (show (Chan.try_send 1 c))
  println (show (Chan.try_send 2 c))
  println (show (Chan.try_recv c))
  Chan.close c
  println (show (Chan.try_send 3 c))
  println (show (Chan.try_recv c))
  let y = Chan.new 1
  let _ = Chan.try_send 9 y
  println (show (Chan.try_recv c))
  println (show (Chan.try_recv y))
  let z = Chan.new (-1)
  println (show (Chan.try_send 4 z))
  let outer = Chan.new 1
  let inner = Chan.new 1
  let _ = Chan.try_send 5 inner
  let _ = Chan.try_send inner outer
  match Chan.try_recv outer with
    | Chan.Got i -> println (show (Chan.try_recv i))
    | _ -> println "unreachable"
