-- A data type whose only constructor has no fields has a single value, so its pattern tests nothing, whether
-- it appears in a parameter, a `let` or a `match` arm.
data Token =
  | Token

spend : Token -> Int -> Int
spend Token n = n + 1

main : Unit -> <IO> Unit
main () =
  let Token = Token
  let t = Token
  let n = match t with
    | Token -> spend t 41
  println (show n)
