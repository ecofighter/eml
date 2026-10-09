-- `return` を省いた handler は、部分を共有する型の状態を捨てる。状態の型を表示するのは捨てたことを報告するとき
-- だけなので、誤りのないプログラムでは型を木として書き出さない。
ident : a -> a
ident x = x

effect Ask where
  ask : Unit -> Int

main : Unit -> <IO> Unit
main () =
  let f0 = ident
  let f1 = f0 ident
  let f2 = f1 ident
  let f3 = f2 ident
  let f4 = f3 ident
  let f5 = f4 ident
  let f6 = f5 ident
  let f7 = f6 ident
  let f8 = f7 ident
  let f9 = f8 ident
  let f10 = f9 ident
  let f11 = f10 ident
  let f12 = f11 ident
  let f13 = f12 ident
  let f14 = f13 ident
  let f15 = f14 ident
  let f16 = f15 ident
  let f17 = f16 ident
  let f18 = f17 ident
  let f19 = f18 ident
  let f20 = f19 ident
  let f21 = f20 ident
  let f22 = f21 ident
  let f23 = f22 ident
  let f24 = f23 ident
  let f25 = f24 ident
  let f26 = f25 ident
  let f27 = f26 ident
  let f28 = f27 ident
  let f29 = f28 ident
  let f30 = f29 ident
  let f31 = f30 ident
  let f32 = f31 ident
  let f33 = f32 ident
  let f34 = f33 ident
  let f35 = f34 ident
  let f36 = f35 ident
  let f37 = f36 ident
  let f38 = f37 ident
  let f39 = f38 ident
  let f40 = f39 ident
  let n =
    handle ask () * 2 from f0 with
      | ask () k s -> k (f40 21) s
  println (show n)
