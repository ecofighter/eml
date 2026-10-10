-- E4001: missing list shapes are written as lists (`[]`, `[_]`) or with right-associative `::`.
first : List Int -> Int
first xs = match xs with
  | [a] -> a

second : List (List Int) -> Int
second xs = match xs with
  | [] -> 0
  | [] :: _ -> 0

f : List (List Int) -> Int
f xs = match xs with
  | [] -> 0
  | [[]] -> 0
  | _ :: _ :: _ -> 0
  | [_ :: _ :: _] -> 0

g : Option (List Int) -> Int
g o = match o with
  | None -> 0
  | Some [] -> 0
  | Some (_ :: _ :: _) -> 0

h : Option (List Int) -> Int
h o = match o with
  | None -> 0
  | Some (_ :: _) -> 0

k : List (Option Int) -> Int
k xs = match xs with
  | [] -> 0
  | None :: _ -> 0
