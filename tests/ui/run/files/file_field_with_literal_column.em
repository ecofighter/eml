-- `File` を持つ `W` を、リテラルの列を持つ行と値全体を束縛する行で分けても、`File` の参照は増えない。
-- 分解した後も、フィールドは持ち主の `W` に借りたままになる (docs/spec/lir.md の「Perceus」)。
import FileIn

data W =
  | W FileIn.FileIn Int

close_w : W -> <IO> Unit
close_w w = match w with
  | W h _ -> FileIn.close h

f : W -> <IO> Unit
f w = match w with
  | W h 0 -> FileIn.close h
  | other -> close_w other

main : Unit -> <IO> Unit
main () =
  f (W (FileIn.open "input.txt") 0)
  f (W (FileIn.open "input.txt") 1)
  println "done"
