-- E1032: the `pub` signature of `reveal` mentions `Secret`, which is not `pub`.
data Secret = | Secret Int

pub reveal : Int -> Secret
reveal n = Secret n

main : Unit -> <IO> Unit
main () = println "x"
