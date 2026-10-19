-- E2002: the row of `show` is empty, so an instance body cannot divide.
data Ratio = | Ratio Int Int

instance Show Ratio where
  show r = match r with
    | Ratio n d -> show (n / d)

main : Unit -> <IO> Unit
main () = println (show (Ratio 6 3))
