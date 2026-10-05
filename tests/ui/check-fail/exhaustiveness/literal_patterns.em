-- E4001: a `match` on string literals has no wildcard arm.
greeting : String -> String
greeting lang = match lang with
  | "en" -> "hello"
  | "ja" -> "konnichiwa"

main : Unit -> <IO> Unit
main () = println (greeting "en")
