-- `var` で書いたエラトステネスのふるい (S14c-3 の完了の条件)。引数 (既定は 20000) より小さい素数の数を書く。
-- `sieve_by_hand.em` は、同じ計算で `MutArray` を末尾再帰のループに線形に受け渡す版である
sieve : Int -> <Panic> Array Bool
sieve n =
  var marks = MutArray.make n True
  for 2 n (fn i ->
    if marks[i] then
      var j = i * i
      while (fn () -> j < n) (fn () ->
        marks[j] <- False
        j <- j + i))
  MutArray.freeze marks

count : Array Bool -> <Panic> Int
count marks =
  var c = 0
  for 2 (Array.length marks) (fn i -> if marks[i] then c <- c + 1)
  c

main : Unit -> <IO, Panic> Unit
main () = println (show (count (sieve (size 20000))))

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
usage () = panic "usage: sieve_var [SIZE]"
