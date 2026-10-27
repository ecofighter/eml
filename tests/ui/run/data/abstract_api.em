-- 宣言したモジュールの中では、抽象型を普通の型と同じに使える。非公開の型をフィールドに持つ `pub abstract data` は
-- E1032 にならず、導出した `Show` は中身を表示する (docs/spec/declarations.md の「`pub`」)。
data Secret = | Secret Int

pub abstract data Sealed = | Sealed Secret

seal : Int -> Sealed
seal n = Sealed (Secret n)

unseal : Sealed -> Int
unseal (Sealed (Secret n)) = n

pub abstract data Counter = | Counter Int
  deriving (Show)

main : Unit -> <IO, Panic> Unit
main () =
  println (show (unseal (seal 41) + 1))
  println (show (Counter 3))
