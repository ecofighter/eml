-- E1001: `shout` is not defined, and nothing else is reported.
main : Unit -> <IO> Unit
main () = println (shout "hi")
