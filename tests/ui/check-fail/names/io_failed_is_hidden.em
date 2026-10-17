-- The operation of the extern effect `IO` is hidden outside the Prelude: plain, qualified and as a clause head.
f : Unit -> <IO> Int
f () = io_failed NotFound "x"

g : Unit -> <IO> Int
g () = Prelude.io_failed NotFound "x"

h : Unit -> Int
h () =
  handle 1 with
    | ctl io_failed kind message -> 0
