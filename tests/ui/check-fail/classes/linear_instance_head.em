-- E2010: a hand-written instance for a type with a linear field. The method has a default, so the
-- instance has no body that would misuse the field.
class Size a where
  size : a -> Int
  size _ = 1

data Handle = | Handle Fs.File

instance Size Handle

main : Unit -> <IO> Unit
main () = ()
