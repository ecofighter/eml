-- 配列の変数の要素は `Unr` でなければならない。`File` を書き込むと E3001
main : Unit -> <IO, Panic> Unit
main () =
  var a = MutArray.empty ()
  a[0] := Fs.open "input.txt"
  drop a
