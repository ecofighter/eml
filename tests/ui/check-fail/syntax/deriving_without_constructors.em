-- E0011: an alternatives block needs a constructor; `deriving` alone does not count. Only the
-- syntax error is reported, not errors from the type it would declare.
data Never =
  deriving Show

describe : Never -> String
describe n = show n

main : Unit -> <IO> Unit
main () = println "x"
