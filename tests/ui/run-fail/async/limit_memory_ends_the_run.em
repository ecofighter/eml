-- max-bytes: 16777216
-- メモリの上限は、タスクの中でも`try_io`では捕まえられず、runを止める (docs/spec/vm.mdの「上限と割り込み」)。
import Async (Async)

build : Int -> List Int -> <Panic> List Int
build n acc = if n == 0 then acc else build (n - 1) (n :: acc)

grow : List Int -> <Panic> Int
grow acc = grow (build 1000 acc)

main : Unit -> <IO, Panic, Async> Unit
main () =
  println "before"
  match try_io (fn () -> grow []) with
    | Ok n -> println "unreachable \{show n}"
    | Err e -> println "unreachable: \{e}"
