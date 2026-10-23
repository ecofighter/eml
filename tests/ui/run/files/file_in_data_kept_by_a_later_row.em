-- `FileIn` を持つ `H` を入れ子のパターンで分解し、後の行がその `H` を `h` として束縛する。
import FileIn

data H =
  | H FileIn.FileIn String

data W =
  | W H Int

close_h : H -> <IO> String
close_h h = match h with
  | H file name ->
      FileIn.close file
      name

f : W -> <IO> String
f w = match w with
  | W (H file "x") n ->
      FileIn.close file
      "x"
  | W h _ -> close_h h ++ "!"

main : Unit -> <IO> Unit
main () =
  println (f (W (H (FileIn.open "input.txt") "x") 1))
  println (f (W (H (FileIn.open "input.txt") "y") 2))
