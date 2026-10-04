-- 組み込みの関数と演算子のシグネチャ。型は docs/spec/declarations.md の標準の演算子の表と、docs/spec/effects.md の
-- 組み込みの IO に従う。名前と見え方と引数の数は eml_hir::builtin::BUILTINS にある。
println : String -> <IO> Unit
show_int : Int -> String
not : Bool -> Bool
negate : Int -> Int
(+) : Int -> Int -> Int
(-) : Int -> Int -> Int
(*) : Int -> Int -> Int
(/) : Int -> Int -> Int
(%) : Int -> Int -> Int
(==) : Int -> Int -> Bool
(!=) : Int -> Int -> Bool
(<) : Int -> Int -> Bool
(<=) : Int -> Int -> Bool
(>) : Int -> Int -> Bool
(>=) : Int -> Int -> Bool
(++) : String -> String -> String
(>>) : (a -> <e> b) -> (b -> <e> c) -> a -> <e> c
(<<) : (b -> <e> c) -> (a -> <e> b) -> a -> <e> c
