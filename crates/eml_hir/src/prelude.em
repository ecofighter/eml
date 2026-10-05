-- 組み込みの型 `Bool` と、関数と演算子のシグネチャ。型は docs/spec/declarations.md の標準の演算子の表と、
-- docs/spec/effects.md の組み込みの IO に従う。関数の名前と見え方と引数の数は eml_hir::builtin::BUILTINS にある。
-- 標準の演算子の fixity (docs/spec/declarations.md の表)。`&&`、`||`、`|>`、`<|` はシグネチャがなく HIR で脱糖し、
-- `::` は S2 のリストの演算子だが、fixity は Prelude の中でだけ宣言できる。
infixr 0 <|
infixl 1 |>
infixr 2 ||
infixr 3 &&
infix 4 ==, !=, <, <=, >, >=
infixr 5 ++, ::
infixl 6 +, -
infixl 7 *, /, %
infixr 9 >>, <<

data Bool =
  | False
  | True

println : String -> <IO> Unit
open : String -> <IO> File
read_all : File -> <IO> (File, String)
close : File -> <IO> Unit
show_int : Int -> String
not : Bool -> Bool
negate : Int -> Int
(+) : Int -> Int -> Int
(-) : Int -> Int -> Int
(*) : Int -> Int -> Int
(/) : Int -> Int -> Int
(%) : Int -> Int -> Int
-- `==` と `!=` で比べられるのは `Int`、`String`、`Bool` で、どれで比べるかは型検査が引数の型から決める
(==) : a -> a -> Bool
(!=) : a -> a -> Bool
(<) : Int -> Int -> Bool
(<=) : Int -> Int -> Bool
(>) : Int -> Int -> Bool
(>=) : Int -> Int -> Bool
(++) : String -> String -> String
(>>) : (a -> <e> b) -> (b -> <e> c) -> a -> <e> c
(<<) : (b -> <e> c) -> (a -> <e> b) -> a -> <e> c
