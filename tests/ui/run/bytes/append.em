-- `Bytes.append a b` は `a` の後ろに `b` を足す。左を一意に持つ組み立てでも、共有した左でも結果は変わらない。
import Bytes

build : Int -> Bytes -> <Panic> Bytes
build n acc = if n == 0 then acc else build (n - 1) (Bytes.append acc (Bytes.of_string "ab"))

main : Unit -> <IO, Panic> Unit
main () =
  let left = Bytes.of_string "xy"
  let both = Bytes.append left (Bytes.of_string "z")
  println (show (Bytes.length left))
  println (show (Bytes.length both))
  println (show (both == Bytes.of_string "xyz"))
  println (show (Bytes.append both both == Bytes.of_string "xyzxyz"))
  let big = build 1000 (Bytes.empty ())
  println (show (Bytes.length big))
  println (show (Bytes.slice 0 4 big == Bytes.of_string "abab"))
