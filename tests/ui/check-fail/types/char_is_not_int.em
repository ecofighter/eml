-- E2006: `Char` has no `Num` instance. E2001: a `Char` is not an `Int`.
sum : Unit -> <Panic> Char
sum () = 'a' + 'b'

same : Bool
same = 'a' == 1
