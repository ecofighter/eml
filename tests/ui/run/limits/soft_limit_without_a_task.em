-- max-bytes: 268435456 soft 4194304
-- タスクのない run は、柔らかい上限を越えても止まらない。旗を立てる先のタスクがなく、硬い上限だけが効く
-- (docs/spec/vm.md の「上限と割り込み」)。
build : Int -> List Int -> <Panic> List Int
build n acc = if n == 0 then acc else build (n - 1) (n :: acc)

length : List Int -> Int -> <Panic> Int
length xs n = match xs with
  | [] -> n
  | _ :: rest -> length rest (n + 1)

main : Unit -> <IO, Panic> Unit
main () = println (show (length (build 300000 []) 0))
