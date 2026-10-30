-- 物差しの床。Net と Async を読み込んで、何もしない run を1回する (コンパイルと起動の分)
import Async (Async)
import Async
import Bytes
import Net
import Env
import String

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen "127.0.0.1" 0
  Net.close_listener l
  println "base done \{show (int_arg 0 0)}")

-- 引数の読み取り。各プログラムが1ファイルで動くように、同じ定義を写して持つ
int_arg : Int -> Int -> <IO, Panic> Int
int_arg i default =
  match nth i (Env.args ()) with
    | Some s ->
      match String.to_int s with
        | Some n -> n
        | None -> default
    | None -> default

str_arg : Int -> String -> <IO, Panic> String
str_arg i default =
  match nth i (Env.args ()) with
    | Some s -> s
    | None -> default

nth : Int -> List String -> <Panic> Option String
nth i xs =
  match xs with
    | Nil -> None
    | x :: rest -> if i == 0 then Some x else nth (i - 1) rest
