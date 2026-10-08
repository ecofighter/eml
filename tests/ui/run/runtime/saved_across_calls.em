-- Values live across calls are saved in the call's frame: a string passed to a call and used again,
-- an integer used after a call, and a call inside a branch whose merge block uses a value from before the `if`.
twice : String -> String
twice s = s ++ s

count : Int -> Int
count n = if n == 0 then 0 else 1 + count (n - 1)

label : Bool -> Int -> String -> String
label b n s =
  let t = if b then twice s else s
  t ++ show_int (n + count 3)

main : Unit -> <IO> Unit
main () =
  let s = "ab"
  let t = twice s
  println (t ++ s)
  let n = 40
  let m = count 2
  println (show_int (n + m))
  println (label True 1 "x")
  println (label False 2 "y")
