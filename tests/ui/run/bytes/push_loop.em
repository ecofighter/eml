-- `push` で `empty` から伸ばす。左を一意に持つので、長さに比例する時間で済む。
import Bytes

fill : Int -> Bytes -> <Panic> Bytes
fill n acc = if n == 0 then acc else fill (n - 1) (Bytes.push 65u8 acc)

main : Unit -> <IO, Panic> Unit
main () =
  let b = fill 1000 (Bytes.empty ())
  println (show (Bytes.length b))
  println (show (Bytes.get 999 b))
  println (show (Bytes.slice 0 3 b))
  let shared = Bytes.push 66u8 (Bytes.of_string "a")
  println (show shared)
