-- `Yield Int` を受けて `Yield String` を出す handler。`render` の `g ()` の `mask` は `Yield@1` で、`gen` が自分で
-- 起こす `Yield Int` は `render` の handler に届く。`gen` の中の `cb ()` の `mask` は `Yield` で、`cb` の
-- `Yield Bool` は `render` の handler と `strings` の handler を飛ばして `bools` に届く (docs/spec/types.md の「推論」)。
effect Yield a where
  yield : a -> Unit

gen : (Unit -> <e> Unit) -> Unit -> <Yield Int | e> Unit
gen cb () =
  yield 1
  cb ()
  yield 2

render : (Unit -> <Yield Int | e> Unit) -> <Yield String | e> Unit
render g =
  handle g () with
    | yield n k ->
      yield "n=\{n}"
      k ()

strings : (Unit -> <Yield String, IO | e> a) -> <IO | e> a
strings action =
  handle action () with
    | yield s k ->
      println "string: \{s}"
      k ()

bools : (Unit -> <Yield Bool, IO | e> a) -> <IO | e> a
bools action =
  handle action () with
    | yield b k ->
      println "bool: \{b}"
      k ()

main : Unit -> <IO> Unit
main () =
  use bools
  use strings
  render (gen (fn () -> yield True))
