-- E2001: リテラルは単相なので、`Int` と `Float` を混ぜた演算は型の誤りになる。
main : Unit -> <IO, Panic> Unit
main () = println (show (1 + 1.5 == 2.5))
