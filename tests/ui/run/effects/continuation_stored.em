-- `multi` の操作の継続を `data` にしまって handle の外へ返し、handle が終わった後に2回再開する。
data Paused =
  | Done Int
  | Paused (Bool -> Paused)

effect Choose where
  multi choose : Unit -> Bool

body : Unit -> <Choose> Int
body () = if choose () then 10 else 20

start : Unit -> Paused
start () =
  handle body () with
    | choose () k -> Paused k
    | return x -> Done x

value_of : Paused -> Int
value_of p =
  match p with
    | Done n -> n
    | Paused _ -> 0

main : Unit -> <IO> Unit
main () =
  match start () with
    | Done _ -> println "not paused"
    | Paused resume_with ->
        println (show_int (value_of (resume_with True)))
        println (show_int (value_of (resume_with False)))
