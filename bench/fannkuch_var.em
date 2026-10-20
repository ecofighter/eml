-- `var` で書いた fannkuch-redux (既定は n = 7、S14c-3 の完了の条件)。n を引数で受ける。配列の変数を3本使う。
-- `fannkuch_by_hand.em` は、同じ計算で状態をすべて末尾再帰の局所関数に受け渡す版である
fannkuch : Int -> <IO, Panic> Unit
fannkuch n =
  var perm1 = MutArray.tabulate n (fn i -> i)
  var perm = MutArray.make n 0
  var count = MutArray.make n 0
  var max_flips = 0
  var checksum = 0
  var parity = 0
  var r = n
  var done = False
  while (fn () -> not done) (fn () ->
    while (fn () -> r != 1) (fn () ->
      count[r - 1] <- r
      r <- r - 1)
    for 0 n (fn i -> perm[i] <- perm1[i])
    var flips = 0
    var k = perm[0]
    while (fn () -> k != 0) (fn () ->
      var lo = 0
      var hi = k
      while (fn () -> lo < hi) (fn () ->
        let t = perm[lo]
        perm[lo] <- perm[hi]
        perm[hi] <- t
        lo <- lo + 1
        hi <- hi - 1)
      flips <- flips + 1
      k <- perm[0])
    if flips > max_flips then max_flips <- flips
    checksum <- checksum + (if parity % 2 == 0 then flips else 0 - flips)
    var more = True
    while (fn () -> more) (fn () ->
      if r == n then
        done <- True
        more <- False
      else
        let perm0 = perm1[0]
        for 0 r (fn i -> perm1[i] <- perm1[i + 1])
        perm1[r] <- perm0
        count[r] <- count[r] - 1
        if count[r] > 0 then more <- False
        else r <- r + 1)
    parity <- parity + 1)
  report n (MutArray.freeze perm1) (MutArray.freeze perm) (MutArray.freeze count) checksum max_flips

-- 3本の配列を受け取って捨て、結果を書く
report : Int -> Array Int -> Array Int -> Array Int -> Int -> Int -> <IO> Unit
report n _ _ _ checksum max_flips =
  println (show checksum)
  println "Pfannkuchen(\{n}) = \{max_flips}"

main : Unit -> <IO, Panic> Unit
main () = fannkuch (size 7)

-- 引数がなければ既定の大きさを使う。`bench/scripts.sh` は大きな大きさを引数で渡す
-- (docs/implementation/benchmarks.md の「スクリプトの計測」)
size : Int -> <IO, Panic> Int
size default =
  match Env.args () with
    | Nil -> default
    | [arg] ->
      match String.to_int arg with
        | Some n -> if n >= 1 then n else usage ()
        | None -> usage ()
    | _ -> usage ()

usage : Unit -> <Panic> Int
usage () = panic "usage: fannkuch_var [SIZE]"
