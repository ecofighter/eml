-- 操作の引数が定数 (`True`、`False`、引数のないコンストラクタ。フィールドを持つコンストラクタと混ざるものも含む)
-- のとき、節の本体を展開すると、その定数で分ける `switch` ができる。optimize は、それを合う case へのジャンプにする
-- (docs/spec/core-ir.md の「handler の複製」)。
import Yield (Yield(..))

data Box a = | Box a

data Level = | Info | Warn

data T = | A | B Int

effect Say where
  say : Level -> Unit

effect Pick where
  pick : T -> Unit

flags : (Int -> <Panic> Int) -> Unit -> <Yield Bool, Panic> Int
flags g () =
  yield True
  yield False
  g 41

levels : Unit -> <Say> Unit
levels () =
  say Info
  say Warn
  say Info

picks : Unit -> <Pick> Unit
picks () =
  pick A
  pick (B 3)

main : Unit -> <IO, Panic> Unit
main () =
  let Box g = Box (fn x -> x + 1)
  let r = handle flags g () from 0 with
    | fn yield b st -> if b then ((), st + 10) else ((), st + 1)
    | return x st -> x * 100 + st
  println (show r)
  handle levels () with
    | fn say l -> match l with
      | Info -> println "info"
      | Warn -> println "warn"
  handle picks () with
    | fn pick t -> match t with
      | A -> println "a"
      | B n -> println "b\{n}"
