-- Records may be written over several lines: a field value may start on the next line, a lambda body ends at the
-- comma at the end of its last line, a trailing comma is allowed, and a record may be the body of a `match` arm.
data Person =
  | Person { name : String, age : Int }
  deriving (Show)

data Hooks =
  | Hooks { label : String, on_save : Int -> Int }

birthday : Person -> Person
birthday p = match p with
  | Person { name, age } -> Person {
      name,
      age = age + 1,
    }

main : Unit -> <IO> Unit
main () =
  let p = Person {
    name =
      "ann",
    age = 30,
  }
  let hooks = Hooks {
    label = "double",
    on_save = fn n ->
      let doubled = n * 2
      doubled,
  }
  println (show (birthday p))
  match hooks with
    | Hooks { label, on_save } -> println "\{label} \{on_save 21}"
