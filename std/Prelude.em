-- 組み込みの型と、関数と演算子のシグネチャ。型は docs/spec/declarations.md の標準の演算子の表と、
-- docs/spec/effects.md の組み込みの IO に従う。`pub` がユーザーからの見え方を決める。`extern` の宣言は、
-- `Prelude.<名前>` の正式な名前で crates/eml_extern の表の行を指し、実装は処理系が持つ。
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

-- 組み込みの型。値の表し方と Kind は extern の表の行が決める
pub extern data Int
pub extern data String
pub extern data Unit
pub extern data File

pub data Bool =
  | False
  | True

-- 組み込みの `IO`。操作のないラベルで、下の extern の関数がこのエフェクトを起こす。extern の関数は handler を
-- 通らずにその場で実行するので、ユーザーは handle できない (docs/spec/effects.md の「組み込みの `IO`」)
pub extern effect IO
pub extern println : String -> <IO> Unit
pub extern open : String -> <IO> File
pub extern read_all : File -> <IO> (File, String)
pub extern close : File -> <IO> Unit

pub extern show_int : Int -> String
pub not : Bool -> Bool
not True = False
not False = True
-- 前置の `-` の脱糖が呼ぶ。`pub` でないので、ユーザーは名前で書けない
extern negate : Int -> Int
pub extern (+) : Int -> Int -> Int
pub extern (-) : Int -> Int -> Int
pub extern (*) : Int -> Int -> Int
pub extern (/) : Int -> Int -> Int
pub extern (%) : Int -> Int -> Int
-- `==` と `!=` で比べられるのは `Int`、`String`、`Bool` である。translate が、型検査の記録した参照ごとの型引数から
-- `eml_types::equality` で比べ方を決め、下の比べ方ごとの extern の呼び出しにする
pub extern (==) : a -> a -> Bool
pub extern (!=) : a -> a -> Bool
extern int_eq : Int -> Int -> Bool
extern int_ne : Int -> Int -> Bool
extern string_eq : String -> String -> Bool
extern string_ne : String -> String -> Bool
extern bool_eq : Bool -> Bool -> Bool
extern bool_ne : Bool -> Bool -> Bool
pub extern (<) : Int -> Int -> Bool
pub extern (<=) : Int -> Int -> Bool
pub extern (>) : Int -> Int -> Bool
pub extern (>=) : Int -> Int -> Bool
pub extern (++) : String -> String -> String
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
