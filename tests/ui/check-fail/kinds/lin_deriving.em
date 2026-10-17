-- E2010: `: Lin` と宣言した型は、`File` と同じく instance を持てない。`deriving` も instance の宣言も誤りになる。
data Token : Lin =
  | Token
  deriving (Eq)

data Key : Lin =
  | Key Int

instance Show Key where
  show _ = "key"

main : Unit -> <IO> Unit
main () = println "unreachable"
