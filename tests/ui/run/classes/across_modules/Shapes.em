pub infixl 6 <+>

pub class Area a where
  area : a -> <Panic> Int
  (<+>) : a -> a -> <Panic> Int
  a <+> b = area a + area b
