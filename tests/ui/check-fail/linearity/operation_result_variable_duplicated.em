-- E3002: `a` appears only in the result of `mk`, so the operation leaves it free and the performer may pick a linear
-- type such as `Fs.File`. The clause must then treat a value of type `a` as linear, and the lambda copies it.
data Box a =
  | Box (a -> (a, a))

effect Mk where
  mk : Unit -> Box a

use_it : Unit -> <Mk, IO> Unit
use_it () =
  match mk () with
    | Box g ->
        let (x, y) = g (Fs.open "input.txt")
        Fs.close x
        Fs.close y

main : Unit -> <IO> Unit
main () =
  handle use_it () with
    | mk () k -> k (Box (fn x -> (x, x)))
    | return u -> u
