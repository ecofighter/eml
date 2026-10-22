-- `File` を捕まえるローカル関数を確かめる。自分を参照しない関数は 1 回だけ呼べる。呼ぶ場所は外側の本体、ラムダ、`use`、
-- handle の本体である。再帰する関数は `File` を捕まえられないので、引数で受け渡す。
-- (docs/spec/linearity.md の「基本の規則」)
effect Ask where
  ask : Unit -> Int

in_body : Unit -> <IO> Unit
in_body () =
  let f = Fs.open "input.txt"
  let finish () = Fs.close f
  finish ()
  println "in body"

in_lambda : Unit -> <IO> Unit
in_lambda () =
  let f = Fs.open "input.txt"
  let finish () = Fs.close f
  let run = fn () -> finish ()
  run ()
  println "in lambda"

with_twice : (Int -> <IO> Unit) -> <IO> Unit
with_twice k = k 1

in_use : Unit -> <IO> Unit
in_use () =
  let f = Fs.open "input.txt"
  let finish n = Fs.close f
  use n <- with_twice
  finish n
  println "in use"

in_handle : Unit -> <IO, Panic> Unit
in_handle () =
  let f = Fs.open "input.txt"
  let finish () =
    Fs.close f
    1
  let n =
    handle finish () + ask () with
      | ctl ask () k -> k 41
  println ("in handle " ++ show (n + 1))

read_twice : Unit -> <IO, Panic> Unit
read_twice () =
  let go f n =
    if n == 0 then Fs.close f
    else
      let (text, f) = Fs.read_all f
      go f (n - 1)
  go (Fs.open "input.txt") 3
  println "passed along"

main : Unit -> <IO, Panic> Unit
main () =
  in_body ()
  in_lambda ()
  in_use ()
  in_handle ()
  read_twice ()
