-- A user instance of the Prelude class Num: componentwise arithmetic, and the prefix `-` calling its
-- `negate`. The Int instances of Num and Div still give the built-in results.
data V2 =
  | V2 Int Int

instance Num V2 where
  (V2 a b) + (V2 c d) = V2 (a + c) (b + d)
  (V2 a b) - (V2 c d) = V2 (a - c) (b - d)
  (V2 a b) * (V2 c d) = V2 (a * c) (b * d)
  negate (V2 a b) = V2 (negate a) (negate b)

instance Show V2 where
  show (V2 a b) = "V2 " ++ show_prec 7 a ++ " " ++ show_prec 7 b

main : Unit -> <IO, Panic> Unit
main () =
  let v = V2 1 2
  let w = V2 10 20
  println (show (v + w))
  println (show (w - v * v))
  println (show (-v))
  println (show (7 / 2, 7 % 2, -(3)))
