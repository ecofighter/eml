-- E0004: row の型引数を持つ `data` は、まだ instance を持てない。instance の頭と `deriving` のどちらも誤りになる。
data Gen <e> =
  | Gen Int
  deriving (Eq)

data Task <e> =
  | Task Int

instance Show (Task <e>) where
  show _ = "task"

main : Unit -> <IO> Unit
main () = println "unreachable"
