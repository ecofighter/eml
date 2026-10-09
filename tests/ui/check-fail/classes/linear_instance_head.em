-- E2010: a hand-written instance for a type with a linear field. Only E2010 is reported: the body
-- of `size` drops the linear value, but the bodies of such an instance are not checked for
-- linearity.
class Size a where
  size : a -> Int

data Handle = | Handle Fs.File

instance Size Handle where
  size _ = 0

main : Unit -> <IO> Unit
main () = ()
