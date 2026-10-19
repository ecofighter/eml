pub infixl 6 <+>

pub double : Int -> <Panic> Int
double x = x * 2

pub (<+>) : Int -> Int -> <Panic> Int
a <+> b = a - b
