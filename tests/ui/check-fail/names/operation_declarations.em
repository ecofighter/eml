-- E1007, E1008, E1003: a row on the outermost arrows of an operation, an operation that is not a function, a
-- `never` operation whose result is not a free type variable, and an operation named like a function.
effect Bad where
  with_row : Int -> <IO> Int
  constant : Int
  never stop : a -> a
  helper : Int -> Int

helper : Int -> Int
helper x = x
