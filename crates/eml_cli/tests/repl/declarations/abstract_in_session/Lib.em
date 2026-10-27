pub abstract data Sealed = | Sealed Int

pub make : Int -> Sealed
make n = Sealed n

pub peek : Sealed -> Int
peek (Sealed n) = n
