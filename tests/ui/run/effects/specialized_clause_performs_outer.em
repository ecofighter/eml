-- 内側の `Yield Int` の handler の `fn` の節が、外側の `yield` を起こす。節を展開した後も、その `yield` は外側の
-- `Yield.for_each` に届く (docs/superpowers/specs/2026-10-18-s13e-specialization-design.md の「意味が変わらない理由」)。
import Yield (Yield(..))

nums : Unit -> <Yield Int, Panic> Unit
nums () =
  let go i =
    if i > 3 then ()
    else
      yield i
      go (i + 1)
  go 1

scale : (Unit -> <Yield Int, Panic> Unit) -> <Yield Int, Panic> Unit
scale producer =
  handle producer () with
    | fn yield x -> yield (x * 10)

main : Unit -> <IO, Panic> Unit
main () = Yield.for_each (fn () -> scale nums) (fn x -> println "\{x}")
