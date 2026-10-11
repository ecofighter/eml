-- `File` を持つ `W` を、リテラルの列を持つ行と値全体を束縛する行で分けても、`File` の参照は増えない。
-- 分解した後も、フィールドは持ち主の `W` に借りたままになる (docs/superpowers/specs/2026-10-11-s8b-ownership-design.md の「`File` の実行時の検査」)。
data W =
  | W Fs.File Int

close_w : W -> <IO> Unit
close_w w = match w with
  | W h _ -> Fs.close h

f : W -> <IO> Unit
f w = match w with
  | W h 0 -> Fs.close h
  | other -> close_w other

main : Unit -> <IO> Unit
main () =
  f (W (Fs.open "input.txt") 0)
  f (W (Fs.open "input.txt") 1)
  println "done"
