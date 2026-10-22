-- 引数と環境変数 (docs/spec/stdlib.md)。入力は env.args と env.env で与える
import Env

main : Unit -> <IO, Panic> Unit
main () =
  println (show (Env.args ()))
  println (show (Env.get "GREETING"))
  println (show (Env.get "EMPTY"))
  println (show (Env.get "MISSING"))
