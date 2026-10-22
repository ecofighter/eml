-- 標準入力 (docs/spec/stdlib.md)。入力は stdin.stdin で与え、最後の行は行の終わりを持たない
main : Unit -> <IO, Panic> Unit
main () =
  let h = show_lines 2 (Stdin.open ())
  let (rest, h) = Stdin.read_all h
  println (show rest)
  let (last, h) = Stdin.read_line h
  println (show last)
  let (empty, h) = Stdin.read_all h
  println (show empty)
  Stdin.close h

-- ハンドルを通しながら `n` 行を読んで出す
show_lines : Int -> Stdin.Stdin -> <IO, Panic> Stdin.Stdin
show_lines n h =
  if n == 0 then h
  else
    let (line, h) = Stdin.read_line h
    println (show line)
    show_lines (n - 1) h
