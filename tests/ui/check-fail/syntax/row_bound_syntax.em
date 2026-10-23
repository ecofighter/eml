-- E0011: 上限に書けるのは `never` だけで、`never` は row 変数の前にしか置けない。
once_bound : (Unit -> <once e> Int) -> <e> Int
once_bound f = f ()

bound_label : (Unit -> <never Panic | e> Int) -> <Panic | e> Int
bound_label f = f ()

main : Unit -> <IO> Unit
main () = println "unreachable"
