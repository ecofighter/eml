-- E2009: the element type of `[]` is never decided, so `display` cannot pick an instance.
empty : Unit -> String
empty () = "\{[]}"
