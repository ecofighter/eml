-- 組み込みの型と、関数と演算子のシグネチャ。型は docs/spec/declarations.md の標準の演算子の表と、
-- docs/spec/effects.md の組み込みの IO に従う。`pub` がユーザーからの見え方を決める。等式のないシグネチャは intrinsic で、
-- 実装は eml_core_ir が名前から引く。
-- 標準の演算子の fixity (docs/spec/declarations.md の表)。
pub infixr 0 <|
pub infixl 1 |>
pub infixr 2 ||
pub infixr 3 &&
pub infix 4 ==, !=, <, <=, >, >=
pub infixr 5 ++
pub infixl 6 +, -
pub infixl 7 *, /, %
pub infixr 9 >>, <<

-- 組み込みの型。`=` のない `data` は Prelude では intrinsic の型で、値の表し方は処理系が決める
pub data Int
pub data String
pub data Unit
pub data File

pub data Bool =
  | False
  | True

-- 組み込みの `IO`。操作は実行時がその場で処理するので、ユーザーは handle できない (docs/spec/effects.md の「組み込みの `IO`」)
pub effect IO where
  println : String -> Unit
  open : String -> File
  read_all : File -> (File, String)
  close : File -> Unit

pub show_int : Int -> String
pub not : Bool -> Bool
not True = False
not False = True
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
f >> g = fn x -> g (f x)
pub (<<) : (b -> <e> c) -> (a -> <e> b) -> a -> <e> c
f << g = fn x -> f (g x)
-- HIR が `if` に脱糖するので、二項演算では本体を呼ばない。短絡して評価するためである
pub (&&) : Bool -> Bool -> Bool
a && b = if a then b else False
pub (||) : Bool -> Bool -> Bool
a || b = if a then True else b
-- 左辺を最初の引数に取る関数である。引数を左から評価するので、`x |> f a` は `x`、`f`、`a` の順に評価する
-- (docs/spec/expressions.md の「関数適用」)
pub (|>) : a -> (a -> <e> b) -> <e> b
x |> f = f x
pub (<|) : (a -> <e> b) -> a -> <e> b
f <| x = f x
