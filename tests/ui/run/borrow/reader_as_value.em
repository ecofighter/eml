-- 文字列を読むだけの関数を、関数の値として使い、部分適用する。値としての参照は所有した引数を受ける写しを指し、
-- 直接の呼び出しは借りる側の元の関数を呼ぶ (docs/spec/lir.md の「関数の ABI」)。
import String

pick : String -> String -> String
pick s other = other ++ show (String.byte_length s)

apply_all : List (String -> String) -> String -> String
apply_all fs x =
  match fs with
    | [] -> x
    | f :: rest -> apply_all rest (f x)

each_pick : (String -> String -> String) -> List String -> String -> String
each_pick f words other =
  match words with
    | [] -> other
    | w :: rest -> each_pick f rest (f w other)

main : Unit -> <IO, Panic> Unit
main () =
  let s = "banana" ++ "!"
  println (each_pick pick [s, "ab" ++ "c", "xyz"] "-")
  let partials = [pick s, pick ("a" ++ "b"), pick s]
  println (apply_all partials "-")
  println (pick s "direct")
  println s
