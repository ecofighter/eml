-- 箱を使い回して、フィールドをほかの位置へ動かす。動いたフィールドはどれも書き直す。走査する語の形と生の語の形の
-- 両方で確かめる。
data P a = | P a a

data T = | T String String String

data Q = | Q Int Int Int

swap_n : Int -> P a -> P a
swap_n n p =
  if n == 0 then p
  else
    match p with
      | P a b -> swap_n (n - 1) (P b a)

rotate_n : Int -> T -> T
rotate_n n t =
  if n == 0 then t
  else
    match t with
      | T a b c -> rotate_n (n - 1) (T c a b)

rotate_q : Int -> Q -> Q
rotate_q n q =
  if n == 0 then q
  else
    match q with
      | Q a b c -> rotate_q (n - 1) (Q c a b)

show_p : P String -> String
show_p p = match p with | P a b -> "P \{a} \{b}"

show_t : T -> String
show_t t = match t with | T a b c -> "T \{a} \{b} \{c}"

show_q : Q -> String
show_q q = match q with | Q a b c -> "Q \{a} \{b} \{c}"

main : Unit -> <IO> Unit
main () =
  let n = List.length ["x"]
  println (show_p (swap_n n (P "a" "b")))
  println (show_p (swap_n (n + 1) (P "a" "b")))
  println (show_t (rotate_n n (T "a" "b" "c")))
  println (show_t (rotate_n (n + 1) (T "a" "b" "c")))
  println (show_t (rotate_n (n + 2) (T "a" "b" "c")))
  println (show_q (rotate_q n (Q 1 2 3)))
  println (show_q (rotate_q (n + 1) (Q 1 2 3)))
