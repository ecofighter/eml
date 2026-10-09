pub infixl 6 <+>

pub class Area a where
  area : a -> Int
  (<+>) : a -> a -> Int
  a <+> b = area a + area b
