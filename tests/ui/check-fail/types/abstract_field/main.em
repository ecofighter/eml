-- E2021: 抽象型のフィールドは、外のモジュールで射影も更新もできない。更新のフィールドが2つでも誤りは1つである
-- (docs/spec/modules.md の「公開の範囲」)。
import Lib

main : Unit -> <IO, Panic> Unit
main () =
  let c = Lib.config "a"
  println c.name
  let d = { c | name = "x", size = 2 }
  println (Lib.name_of d)
