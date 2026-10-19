-- E3002: `: Lin` と宣言した型の値は、フィールドがなくても2回使えない。
data Token : Lin =
  | Token

spend : Token -> Int
spend t =
  match t with
    | Token -> 1

twice : Token -> <Panic> Int
twice t = spend t + spend t

main : Unit -> <IO, Panic> Unit
main () = println (show (twice Token))
