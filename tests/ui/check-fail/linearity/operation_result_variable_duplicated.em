-- E3002: `a` appears only in the result of `mk`, so the operation leaves it free and the performer may pick a linear
-- type such as `FileIn.FileIn`. The clause must then treat a value of type `a` as linear, and the lambda copies it.
import FileIn

data Box a =
  | Box (a -> (a, a))

effect Mk where
  mk : Unit -> Box a

use_it : Unit -> <Mk, IO> Unit
use_it () =
  match mk () with
    | Box g ->
        let (x, y) = g (FileIn.open "input.txt")
        FileIn.close x
        FileIn.close y

main : Unit -> <IO> Unit
main () =
  handle use_it () with
    | ctl mk () k -> k (Box (fn x -> (x, x)))
    | return u -> u
