-- E1044: only the Prelude may declare the constructor `::`.
data Seq a =
  | Empty
  | a :: Seq a

main : Unit -> <IO> Unit
main () = println "done"
