-- 組み込みの型と、関数と演算子のシグネチャ。型は docs/spec/declarations.md の標準の演算子の表と、
-- docs/spec/effects.md の組み込みの IO に従う。`pub` がユーザーからの見え方を決める。`extern` の宣言は、
-- `Prelude.<名前>` の正式な名前で crates/eml_extern の表の行を指し、実装は処理系が持つ。instance の `extern` は
-- `Prelude.<クラス> <型>.<メソッド>` の名前で行を指す。
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

pub data Bool =
  | False
  | True

-- 比較の結果。`compare` の extern の行は、この宣言の順のタグ (`LT` が 0) を返す (eml_core_ir のテストが確かめる)
pub data Ordering =
  | LT
  | EQ
  | GT

-- 組み込みの `IO`。操作のないラベルで、下の extern の関数がこのエフェクトを起こす。extern の関数は handler を
-- 通らずにその場で実行するので、ユーザーは handle できない (docs/spec/effects.md の「組み込みの `IO`」)
pub extern effect IO
pub extern println : String -> <IO> Unit

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

pub class Eq a where
  (==) : a -> a -> Bool
  (!=) : a -> a -> Bool
  x != y = not (x == y)

pub class Eq a => Ord a where
  compare : a -> a -> Ordering
  (<) : a -> a -> Bool
  x < y = match compare x y with
    | LT -> True
    | _ -> False
  (<=) : a -> a -> Bool
  x <= y = match compare x y with
    | GT -> False
    | _ -> True
  (>) : a -> a -> Bool
  x > y = match compare x y with
    | GT -> True
    | _ -> False
  (>=) : a -> a -> Bool
  x >= y = match compare x y with
    | LT -> False
    | _ -> True

-- `show` を必須にするのは、`show_prec` と互いの既定にすると、どちらも書かない instance が止まらなくなるため
pub class Show a where
  show : a -> String
  show_prec : Int -> a -> String
  show_prec _ x = show x

instance Eq Int where
  extern (==)
  extern (!=)

instance Ord Int where
  extern compare
  extern (<)
  extern (<=)
  extern (>)
  extern (>=)

-- 負の数は、関数適用の引数の位置 (優先度 7 より強い) で括弧に入れる (Haskell の `showsPrec` と同じ)
instance Show Int where
  extern show
  show_prec d n = if d > 6 && n < 0 then "(" ++ show n ++ ")" else show n

instance Eq String where
  extern (==)
  extern (!=)

instance Ord String where
  extern compare

instance Show String where
  extern show

instance Eq Bool where
  extern (==)
  extern (!=)

-- Task 9 で `deriving` に置き換える
instance Ord Bool where
  compare False True = LT
  compare True False = GT
  compare _ _ = EQ

instance Show Bool where
  show False = "False"
  show True = "True"

instance Eq Ordering where
  LT == LT = True
  EQ == EQ = True
  GT == GT = True
  _ == _ = False

instance Ord Ordering where
  compare a b = compare (ordinal a) (ordinal b)

instance Show Ordering where
  show LT = "LT"
  show EQ = "EQ"
  show GT = "GT"

ordinal : Ordering -> Int
ordinal LT = 0
ordinal EQ = 1
ordinal GT = 2

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
