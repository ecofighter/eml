-- docs/spec/linearity.md の「暗黙の後始末」: FileIn を持つ Either は線形で、使わないと E3003 になる
import FileIn

leak : Unit -> <IO> Unit
leak () =
  let e = (Right 1 : Either FileIn.FileIn Int)
  println "unused"

main : Unit -> <IO> Unit
main () = leak ()
