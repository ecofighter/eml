pub data Row = | Row String Int

pub make : String -> Int -> Row
make name n = Row name (n * 10)

pub show : Row -> String
show (Row name n) = name ++ "=" ++ show_int n
