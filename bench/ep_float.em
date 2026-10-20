-- `Float` の結果と `Int` の状態を持つ操作を、特殊化しない handler の下で10万回起こす (S14e-1 の基準)。
-- 経路に `mask` の付いた呼び出しがあるので、handler への特殊化は断られ、EP の道 (evidence passing) に残る
-- (docs/spec/core-ir.md の「handler への特殊化」)。`evidence_steps` が 0 より大きい。S14e の規約は、EP の道の
-- 操作の結果と状態を型に合わせた Repr で運ぶので、その費用を見る (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の「足す計測のプログラム」)
effect Gauge where
  read : Int -> Float

masked : (Unit -> <e> a) -> <Gauge | e> a
masked f = f ()

step : Int -> <Gauge, Panic> Float
step i = read i

loop : Int -> Int -> Float -> <Gauge, Gauge, Panic> Float
loop i n acc =
  if i > n then acc
  else loop (i + 1) n (acc + masked (fn () -> step i))

run : Int -> (Unit -> <Gauge | e> a) -> <Panic | e> a
run init action =
  handle action () from init with
    | fn read i st -> (Float.of_int i * 0.5, st + 1)
    | return x st -> x

main : Unit -> <IO, Panic> Unit
main () = println (show (run 0 (fn () -> run 0 (fn () -> loop 1 100000 0.0))))
