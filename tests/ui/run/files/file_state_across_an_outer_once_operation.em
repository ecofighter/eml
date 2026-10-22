-- 内側の handler の状態が `File` で、本体が外側の once の操作を起こす。`prompt$` はほかの宛先の yield (状態 3) として状態を
-- 取り出し、`rewind$` のクロージャに入れる。再開すれば `return` の節が閉じ、`drop k` ならクロージャの解放で閉じる。
import Fs

effect Tick where
  tick : Unit -> Unit

effect Ask where
  ask : Unit -> Int

body : Unit -> <Tick, Ask> Int
body () =
  tick ()
  let n = ask ()
  tick ()
  n

inner : Unit -> <Ask, IO> Int
inner () =
  handle body () from Fs.open "input.txt" with
    | ctl tick () k f -> k () f
    | return x f ->
        Fs.close f
        x

main : Unit -> <IO> Unit
main () =
  let r = handle inner () with
    | ctl ask () k -> k 3
  println (show r)
  let r2 = handle inner () with
    | ctl ask () k ->
        drop k
        42
  println (show r2)
