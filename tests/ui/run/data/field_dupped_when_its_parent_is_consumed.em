-- 分解した途中の値 `i` を呼び出しに消費し、その子 `s` を後でも使う。`i` が必要なので Perceus は `s` を `dup` で取り、
-- 根の `o` は `release` で解く (docs/spec/lir.md の「持ち主を解く」の 3.)。
data Inner =
  | Inner String Int

data Outer =
  | Outer Inner Int

keep : Inner -> Int
keep i = match i with
  | Inner _ n -> n

go : Outer -> <Panic> String
go o = match o with
  | Outer i m -> match i with
      | Inner s n -> let k = keep i in s ++ show (k + n + m)

main : Unit -> <IO, Panic> Unit
main () = println (go (Outer (Inner "z" 1) 2))
