-- 文字列を読むだけの関数を、操作を2回再開するhandlerの下で呼ぶ。文字列は継続の中で生き残り、
-- 読むだけの関数は再開のたびにその文字列をもう一度読む。関数の値はデータに通し、handlerの特殊化で
-- handleが消えないようにする。
import String

effect Choice where
  multi choose : Unit -> Bool

letters : String -> Int -> <Panic> Int
letters s i =
  if i >= String.byte_length s then 0
  else 1 + letters s (i + 1)

measure : String -> <Panic> Int
measure s = letters s 0

weigh : String -> <Choice, Panic> Int
weigh s =
  let before = measure s
  let n = if choose () then 1 else 2
  before + n * 100 + measure s

data Box a = | Box a

hide : Int -> a -> <Panic> Box a
hide n x = if n <= 0 then Box x else hide (n - 1) x

both : (String -> <Choice, Panic> Int) -> String -> <Panic> String
both body s =
  handle body s with
    | ctl choose () k -> k True ++ "," ++ k False
    | return n -> show n

main : Unit -> <IO, Panic> Unit
main () =
  let Box body = hide 0 weigh
  let s = "banana" ++ "!"
  println (both body s)
  println (both body ("ab" ++ "c"))
  println s
