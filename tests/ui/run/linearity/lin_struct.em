-- `Lin` のフィールドを持つ構造体。`Reader` は `File` と `Int` の 2 語の構造体で、`Lin` のまま関数の引数と結果を渡り、
-- 最後に1回だけ閉じる。展開しない関数 (再帰する `skip`) に通す形も通す
-- (docs/spec/core-ir.md の「構造体にする型」)。
import Fs
import String

data Reader =
  | Reader Fs.File Int

open_reader : String -> <IO> Reader
open_reader path = Reader (Fs.open path) 0

read_once : Reader -> <IO, Panic> (Reader, String)
read_once (Reader f n) =
  let (text, f) = Fs.read_all f
  (Reader f (n + 1), text)

skip : Int -> Reader -> <Panic> Reader
skip k r = if k == 0 then r else
  let Reader f n = r
  skip (k - 1) (Reader f (n + 10))

close_reader : Reader -> <IO> Int
close_reader (Reader f n) =
  Fs.close f
  n

main : Unit -> <IO, Panic> Unit
main () =
  let (r, text) = read_once (open_reader "input.txt")
  println text
  let (r, again) = read_once (skip 3 r)
  println (show (String.byte_length again))
  println (show (close_reader r))
