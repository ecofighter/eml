-- A tuple or a constructor built only to be matched is taken apart without being allocated.
data Option a =
  | None
  | Some a

add : Int -> Int -> Int
add a b = match (a, b) with
  | (x, y) -> x + y

unwrap : Int -> Int
unwrap x = match Some x with
  | Some y -> y
  | None -> 0

size : Option Int -> Int
size o = match o with
  | None -> 0
  | Some _ -> 1

pick : Bool -> Int
pick c =
  let n = match (if c then Some 10 else Some 20) with
    | Some n -> n
    | None -> 0
  n + 1

whole : Bool -> Int
whole c =
  let n = match (if c then Some 1 else None) with
    | None -> 0
    | x -> size x
  n + 1

data Color =
  | Red
  | Green
  | Blue

score : Bool -> Int
score b =
  let n = match (if b then Red else Green) with
    | Red -> 1
    | _ -> 2
  n + 1

find : Int -> Option Int
find k = if k > 0 then Some k else None

mixed : Bool -> Int -> Int
mixed c k =
  let n = match (if c then Some 1 else find k) with
    | Some v -> v + 10
    | None -> 0
  n + 1

mixed_whole : Bool -> Int -> Int
mixed_whole c k =
  let n = match (if c then Some 1 else find k) with
    | None -> 0
    | x -> size x
  n + 1

main : Unit -> <IO> Unit
main () =
  println (show_int (add 1 2))
  println (show_int (unwrap 7))
  println (show_int (pick True))
  println (show_int (pick False))
  println (show_int (whole True))
  println (show_int (whole False))
  println (show_int (score True))
  println (show_int (score False))
  println (show_int (mixed True 5))
  println (show_int (mixed False 5))
  println (show_int (mixed False 0))
  println (show_int (mixed_whole True 5))
  println (show_int (mixed_whole False 5))
  println (show_int (mixed_whole False 0))
