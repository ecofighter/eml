-- E2009: the element type of `[]` is never decided, so no `Show` instance can be chosen. Annotate it as
-- `([] : List Int)`.
main : Unit -> <IO> Unit
main () = println (show [])
