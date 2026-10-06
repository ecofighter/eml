-- 組み込みの型 `Bool` と、関数と演算子のシグネチャ。型は docs/spec/declarations.md の標準の演算子の表と、
-- docs/spec/effects.md の組み込みの IO に従う。`pub` がユーザーからの見え方を決める。等式のないシグネチャは intrinsic で、
-- 実装は eml_core_ir が名前から引く。
-- 標準の演算子の fixity (docs/spec/declarations.md の表)。fixity は定義に付くので、定義のない `::` の fixity は、S2 で
-- `List` のコンストラクタと一緒に宣言する。
pub infixr 0 <|
pub infixl 1 |>
pub infixr 2 ||
pub infixr 3 &&
pub infix 4 ==, !=, <, <=, >, >=
pub infixr 5 ++
pub infixl 6 +, -
pub infixl 7 *, /, %
pub infixr 9 >>, <<

pub data Bool =
  | False
  | True

pub println : String -> <IO> Unit
pub open : String -> <IO> File
pub read_all : File -> <IO> (File, String)
pub close : File -> <IO> Unit
pub show_int : Int -> String
pub not : Bool -> Bool
-- 前置の `-` の脱糖が呼ぶ。`pub` でないので、ユーザーは名前で書けない
negate : Int -> Int
pub (+) : Int -> Int -> Int
pub (-) : Int -> Int -> Int
pub (*) : Int -> Int -> Int
pub (/) : Int -> Int -> Int
pub (%) : Int -> Int -> Int
-- `==` と `!=` で比べられるのは `Int`、`String`、`Bool` で、どれで比べるかは型検査が引数の型から決める
pub (==) : a -> a -> Bool
pub (!=) : a -> a -> Bool
pub (<) : Int -> Int -> Bool
pub (<=) : Int -> Int -> Bool
pub (>) : Int -> Int -> Bool
pub (>=) : Int -> Int -> Bool
pub (++) : String -> String -> String
pub (>>) : (a -> <e> b) -> (b -> <e> c) -> a -> <e> c
pub (<<) : (b -> <e> c) -> (a -> <e> b) -> a -> <e> c
-- HIR が脱糖する演算子。ユーザーが同じ演算子を定義すれば、普通の呼び出しになる
pub (&&) : Bool -> Bool -> Bool
pub (||) : Bool -> Bool -> Bool
pub (|>) : a -> (a -> <e> b) -> <e> b
pub (<|) : (a -> <e> b) -> a -> <e> b
