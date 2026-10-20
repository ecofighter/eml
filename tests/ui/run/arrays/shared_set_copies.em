-- 共有された `Array Float` への `set` は写すので、古い参照は元の値を保つ。要素の `Float` の箱は
-- 写しが `dup` するので、検査モードで漏れない。
main : Unit -> <IO, Panic> Unit
main () =
  let a = Array.tabulate 3 (fn i -> Float.of_int i + 0.5)
  let b = Array.set a 1 9.5
  println (show (Array.to_list a))
  println (show (Array.to_list b))
