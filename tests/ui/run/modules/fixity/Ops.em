pub infixl 6 <+>
pub infixl 7 <**>

pub (<+>) : Int -> Int -> <Panic> Int
a <+> b = a + b

pub (<**>) : Int -> Int -> <Panic> Int
a <**> b = a * b
