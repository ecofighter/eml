-- E4001: missing list shapes are written as lists (`[]`, `[_]`) or with right-associative `::`.
first : List Int -> Int
first xs = match xs with
  | [a] -> a

second : List (List Int) -> Int
second xs = match xs with
  | [] -> 0
  | [] :: _ -> 0
