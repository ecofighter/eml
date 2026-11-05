-- `Int as int` は C の `int32_t` で、範囲の外の引数は呼ぶ前に `Panic` を上げる。`as` のない `Int` は `int64_t` である。
link on linux "libm.so.6"
link on macos "m"
link on windows "ucrtbase"

extern "C" pure abs : Int as int -> <Panic> Int as int
extern "C" pure llabs : Int -> Int

main : Unit -> <IO, Panic> Unit
main () =
  println (show (abs (-5)))
  println (show (llabs (-5)))
  println (show (try_panic (fn () -> abs 3000000000)))
