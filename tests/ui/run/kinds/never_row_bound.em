-- `<never e>` の上限は、`Panic` と `never` の操作だけのエフェクト (`Fail`) を受け付ける (docs/spec/types.md の「row 変数の多重度の上限」)。
import Fail (Fail(..))

helper : (Unit -> <never e> Int) -> <e> Int
helper f = f ()

main : Unit -> <IO, Panic> Unit
main () =
  let caught = try_panic (fn () -> helper (fn () -> 1 / 0))
  println (show caught)
  let failed = Fail.to_result (fn () -> helper (fn () -> Fail.fail "no"))
  println (show failed)
