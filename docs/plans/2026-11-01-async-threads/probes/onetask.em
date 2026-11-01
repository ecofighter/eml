import Async

main : Unit -> <IO, Panic> Unit
main () = println (show (Async.run (fn () -> Async.await (Async.start (fn () -> 1)))))
