-- Int and String literal patterns, including a negative number and a variable fallback. The String match runs
-- a thousand times, so every literal string built for a comparison must be freed.
describe : Int -> String
describe n = match n with
  | 0 -> "zero"
  | 1 -> "one"
  | -1 -> "minus one"
  | _ -> "many"

greet : String -> String
greet lang = match lang with
  | "en" -> "hello"
  | "ja" -> "konnichiwa"
  | other -> "? " ++ other

bonus : Int -> Int
bonus n = match greet (if n % 2 == 0 then "en" else "fr") with
  | "hello" -> 1
  | _ -> 0

count_hellos : Int -> Int -> Int
count_hellos n acc = if n == 0 then acc else count_hellos (n - 1) (acc + bonus n)

tagged : (String, Int) -> Int
tagged p = match p with
  | ("a", 0) -> 10
  | (s, n) -> n + 100

main : Unit -> <IO> Unit
main () =
  println (describe 0)
  println (describe 1)
  println (describe (-1))
  println (describe 42)
  println (greet "en")
  println (greet "ja")
  println (greet "fr")
  println (show (count_hellos 1000 0))
  println (show (tagged ("a", 0)))
  println (show (tagged ("a", 1)))
  println (show (tagged ("bcd", 2)))
