-- フィールドのない枝とフィールドのある枝が `match` の後で合流し、合流の後で scrutinee をもう一度使う。
data Opt =
  | None
  | Some String

size : Opt -> Int
size o = match o with
  | None -> 0
  | Some s -> 1

f : Opt -> Int
f o =
  let k = match o with
    | None -> 0
    | Some s -> 1
  k + size o

main : Unit -> <IO> Unit
main () = println (show (f None + f (Some "a")))
