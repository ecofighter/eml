-- E2006: `Bits` has instances for `Int` and `Byte` only, so `'a' &&& 'b'` has no `Bits Char`.
main : Unit -> <IO, Panic> Unit
main () = println (show ('a' &&& 'b'))
