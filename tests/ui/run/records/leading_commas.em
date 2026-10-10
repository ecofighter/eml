-- Records may be written with leading commas: the declaration, construction, update and patterns.
data Person =
  | Person
    { name : String
    , age : Int
    }

main : Unit -> <IO> Unit
main () =
  let p =
    Person
      { name = "Ada"
      , age = 36
      }
  let q =
    { p
      | age = 37
      }
  match q with
    | Person
        { name
        , age
        } -> println "\{name} \{age}"
