-- `File` を開いた生産者の `Iter` を途中で `drop` すると、`File` を捕まえた `return` の節のクロージャも継続と一緒に
-- 解放され、`File` が閉じる。検査モードで漏れがないことで確かめる。最後まで引いたときは `return` の節が閉じる。
data Iter <e> a =
  | Done
  | More a (once Unit -> <e> Iter <e> a)

effect Yield a where
  yield : a -> Unit

yield_all : List a -> <Yield a> Unit
yield_all xs =
  match xs with
    | [] -> ()
    | x :: rest ->
        yield x
        yield_all rest

file_lines : String -> <IO, Panic> Iter <IO, Panic> String
file_lines path =
  let f = Fs.open path
  let (text, f) = Fs.read_all f
  handle yield_all (String.lines text) with
    | ctl yield line k -> More line k
    | return () ->
        Fs.close f
        println "closed by the return clause"
        Done

first_line : Iter <IO, Panic> String -> <IO> String
first_line it =
  match it with
    | Done -> "no lines"
    | More line rest ->
        drop rest
        line

print_all : Iter <IO, Panic> String -> <IO, Panic> Unit
print_all it =
  match it with
    | Done -> println "done"
    | More line next ->
        println line
        print_all (next ())

main : Unit -> <IO, Panic> Unit
main () =
  println (first_line (file_lines "lines.txt"))
  print_all (file_lines "lines.txt")
