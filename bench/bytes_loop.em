-- 10 万バイトのバイト列を `push` で作り、`Bytes.get` で先頭から足し上げる。1バイトの読みごとの RC が `rc_increments` に出る。
grow : Int -> Bytes -> <Panic> Bytes
grow n acc = if n == 0 then acc else grow (n - 1) (Bytes.push 65u8 acc)

sum_bytes : Bytes -> Int -> Int -> Int -> <Panic> Int
sum_bytes b i n acc =
  if i == n then acc else sum_bytes b (i + 1) n (acc + Byte.to_int (Bytes.get i b))

main : Unit -> <IO, Panic> Unit
main () =
  let b = grow 100000 (Bytes.empty ())
  println (show (sum_bytes b 0 (Bytes.length b) 0))
