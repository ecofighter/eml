-- `match` の後も生きている記録のフィールドを、`try_io` の下の IO の失敗をまたいで使う。巻き戻しは借りたフィールドを
-- 手放さず、記録とフィールドを1回ずつ解放する。
import Fs

data Person =
  | Person { name : String, city : String }

greet : Person -> <IO> String
greet p =
  match p with
    | Person { name } ->
      Fs.close (Fs.open "missing.txt")
      name ++ " from " ++ p.city

main : Unit -> <IO> Unit
main () =
  let p = Person { name = "Ada", city = "London" }
  match try_io (fn () -> greet p) with
    | Ok s -> println s
    | Err e -> println "greet: \{e}"
