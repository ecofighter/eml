-- `fannkuch_var.em` の計算を、状態をすべて末尾再帰の局所関数に受け渡して手で書いた版。3本の `MutArray` を
-- 線形に受け渡し、ループの間の移りも末尾呼び出しにするので、組を作らない
import MutArray

fannkuch : Int -> <IO, Panic> Unit
fannkuch n =
  let fill_count max_flips checksum parity r perm1 perm count =
        if r != 1 then
          fill_count max_flips checksum parity (r - 1) perm1 perm (MutArray.write count (r - 1) r)
        else copy max_flips checksum parity r 0 perm1 perm count
      and copy max_flips checksum parity r i perm1 perm count =
        if i < n then
          let (v, perm1) = MutArray.read perm1 i
          copy max_flips checksum parity r (i + 1) perm1 (MutArray.write perm i v) count
        else
          let (k, perm) = MutArray.read perm 0
          flip max_flips checksum parity r 0 k perm1 perm count
      and flip max_flips checksum parity r flips k perm1 perm count =
        if k != 0 then reverse max_flips checksum parity r flips 0 k perm1 perm count
        else
          let max_flips = if flips > max_flips then flips else max_flips
          let checksum = checksum + (if parity % 2 == 0 then flips else 0 - flips)
          next max_flips checksum parity r perm1 perm count
      and reverse max_flips checksum parity r flips lo hi perm1 perm count =
        if lo < hi then
          let (t, perm) = MutArray.read perm lo
          let (u, perm) = MutArray.read perm hi
          let perm = MutArray.write (MutArray.write perm lo u) hi t
          reverse max_flips checksum parity r flips (lo + 1) (hi - 1) perm1 perm count
        else
          let (k, perm) = MutArray.read perm 0
          flip max_flips checksum parity r (flips + 1) k perm1 perm count
      and next max_flips checksum parity r perm1 perm count =
        if r == n then report n (MutArray.freeze perm1) (MutArray.freeze perm) (MutArray.freeze count) checksum max_flips
        else
          let (perm0, perm1) = MutArray.read perm1 0
          rotate max_flips checksum parity r 0 perm0 perm1 perm count
      and rotate max_flips checksum parity r i perm0 perm1 perm count =
        if i < r then
          let (v, perm1) = MutArray.read perm1 (i + 1)
          rotate max_flips checksum parity r (i + 1) perm0 (MutArray.write perm1 i v) perm count
        else
          let perm1 = MutArray.write perm1 r perm0
          let (c, count) = MutArray.read count r
          let count = MutArray.write count r (c - 1)
          if c - 1 > 0 then fill_count max_flips checksum (parity + 1) r perm1 perm count
          else next max_flips checksum parity (r + 1) perm1 perm count
  fill_count 0 0 0 n (MutArray.tabulate n (fn i -> i)) (MutArray.make n 0) (MutArray.make n 0)

-- 3本の配列を受け取って捨て、結果を書く
report : Int -> Array Int -> Array Int -> Array Int -> Int -> Int -> <IO> Unit
report n _ _ _ checksum max_flips =
  println (show checksum)
  println "Pfannkuchen(\{n}) = \{max_flips}"

main : Unit -> <IO, Panic> Unit
main () = fannkuch 7
