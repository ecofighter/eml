-- E0011: a pattern matches a fixed string, so it cannot have holes.
greeting : String -> Int
greeting s = match s with
  | "hello \{s}" -> 1
  | _ -> 0
