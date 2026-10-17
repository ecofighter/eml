-- E3001: 相互に呼ぶローカル関数の組の関数は何度も呼ばれうるので、`File` を捕まえられない。`close_at` は自分を参照しない
-- が、`skip` から何度も呼ばれうる。
closes_in_group : Unit -> <IO> Unit
closes_in_group () =
  let f = Fs.open "input.txt"
  let close_at n = if n == 0 then Fs.close f else skip (n - 1)
  and skip n = close_at n
  close_at 3

main : Unit -> <IO> Unit
main () = closes_in_group ()
