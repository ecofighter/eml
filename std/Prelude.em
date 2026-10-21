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
pub extern data Float
pub extern data String
pub extern data Unit

pub data Bool =
  | False
  | True
  deriving (Ord, Show)

-- 比較の結果。`compare` の extern の行は、この宣言の順のタグ (`LT` が 0) を返す (eml_core_ir のテストが確かめる)
pub data Ordering =
  | LT
  | EQ
  | GT
  deriving (Eq, Ord, Show)

-- 組み込みの `IO`。下の extern の関数がこのエフェクトを起こし、handler を通らずにその場で実行する。操作の
-- `io_failed` は Prelude の外から見えず、`try_io` だけが handle する。IO の失敗は、VM がこの操作の節を呼んで
-- `try_io` まで中断する (docs/spec/effects.md の「組み込みの `IO`」)
pub extern effect IO where
  never io_failed : IOErrorKind -> String -> a
pub extern println : String -> <IO> Unit
-- 標準エラーに1行を書く。C の stdio と同じく、標準出力は書き出さない
pub extern eprintln : String -> <IO> Unit

-- 途中のフレームが持っていた値を後始末して、引数を終了コードにして終わる。`try_io` は捕まえない
-- (docs/spec/stdlib.md の「`exit`」)
pub extern exit : Int -> <IO> a

-- 組み込みの `Panic`。値と入力で決まる実行時エラー (整数の桁あふれとゼロ除算、文字列の範囲外の添字) と `panic` が
-- 起こす。`IO` と同じく、操作の `panicked` は Prelude の外から見えず、`try_panic` だけが handle する
-- (docs/spec/effects.md の「組み込みの `Panic`」)
pub extern effect Panic where
  never panicked : String -> a
-- 文言を持って `Panic` を起こす。`exit` と同じく戻らない
pub extern panic : String -> <Panic> a

pub not : Bool -> Bool
not True = False
not False = True

-- 算術のクラス。メソッドの型はクラスで1つなので、`Panic` を起こさない instance も `<Panic>` を持つ。前置の `-` は
-- `negate` を呼ぶ (docs/spec/declarations.md の「Prelude のクラス」)
pub class Num a where
  (+) : a -> a -> <Panic> a
  (-) : a -> a -> <Panic> a
  (*) : a -> a -> <Panic> a
  negate : a -> <Panic> a

pub class Div a where
  (/) : a -> a -> <Panic> a
  (%) : a -> a -> <Panic> a

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

-- `show` を必須にするのは、`show_prec` と互いの既定にすると、どちらも書かない instance が止まらなくなるため。
-- `display` は補間の穴が呼ぶ (docs/spec/declarations.md の「Prelude のクラス」)
pub class Show a where
  show : a -> String
  show_prec : Int -> a -> String
  show_prec _ x = show x
  display : a -> String
  display x = show x

instance Num Int where
  extern (+)
  extern (-)
  extern (*)
  extern negate

instance Div Int where
  extern (/)
  extern (%)

instance Eq Int where
  extern (==)
  extern (!=)

instance Ord Int where
  extern compare
  extern (<)
  extern (<=)
  extern (>)
  extern (>=)

-- `Float` の算術は IEEE 754 のとおりで、`Panic` を起こさない。`%` の結果は割られる数の符号を持つ (C の `fmod`)
instance Num Float where
  extern (+)
  extern (-)
  extern (*)
  extern negate

instance Div Float where
  extern (/)
  extern (%)

instance Eq Float where
  extern (==)
  extern (!=)

-- 比較の演算子は IEEE 754 のとおりで、NaN が絡むと偽になる。`compare` だけは全順序 (`-0.0` は `0.0` より小さく、
-- NaN は最も大きい) で、並べ替えが NaN を含んでも順序として成り立つ。演算が作る NaN はいつも正の quiet NaN なので、
-- どのプラットフォームでも NaN は `inf` より大きい (docs/spec/declarations.md の「Prelude のクラス」)
instance Ord Float where
  extern compare
  extern (<)
  extern (<=)
  extern (>)
  extern (>=)

-- 負の数は、関数適用の引数の位置 (優先度 7 より強い) で括弧に入れる (Haskell の `showsPrec` と同じ)
instance Show Int where
  extern show
  show_prec d n = if d > 6 && n < 0 then "(" ++ show n ++ ")" else show n

-- Python の `repr` と同じ形で書く。`-` で始まる表示 (`-0.0` と `-inf` も) は、`Int` と同じく引数の位置で括弧に入れる
-- (docs/spec/declarations.md の「Prelude のクラス」)
instance Show Float where
  extern show
  show_prec d x = if d > 6 && shows_minus x then "(" ++ show x ++ ")" else show x

-- `show x` が `-` で始まるか。`x < 0.0` は `-0.0` で偽になるので、符号を読む `Pure` の extern で見分ける
extern shows_minus : Float -> Bool

instance Eq String where
  extern (==)
  extern (!=)

instance Ord String where
  extern compare

instance Show String where
  extern show
  display s = s

instance Eq Bool where
  extern (==)
  extern (!=)

pub infixr 5 ::

pub data List a =
  | Nil
  | a :: List a
  deriving (Eq, Ord)

pub data Option a =
  | None
  | Some a
  deriving (Eq, Ord, Show)

pub data Result e a =
  | Err e
  | Ok a
  deriving (Eq, Ord, Show)

-- `try_io` が捕まえる IO の失敗の種類。VM は宣言の順のタグで渡す (docs/spec/stdlib.md の「`try_io`」)
pub data IOErrorKind =
  | NotFound
  | PermissionDenied
  | NotAvailable
  | NotUtf8
  | Other
  deriving (Eq, Show)

pub data IOError =
  | IOError { kind : IOErrorKind, message : String }

-- `show` は導出した `Show` のレコードと同じ形で、`display` は文言だけを返す。導出では `display` を上書きできない
instance Show IOError where
  show e = "IOError { kind = \{show e.kind}, message = \{show e.message} }"
  display e = e.message

-- IO の失敗を `Err` にする。捕まえるのは実行の時点で最も近い `try_io` である (docs/spec/stdlib.md の「`try_io`」)
pub try_io : (Unit -> <IO | e> a) -> <IO | e> Result IOError a
try_io action =
  handle action () with
    | ctl io_failed kind message -> Err (IOError { kind = kind, message = message })
    | return x -> Ok x

-- `Panic` を `Err` にする。捕まえるのは実行の時点で最も近い `try_panic` で、`Err` は誤りの文言を持つ
-- (docs/spec/stdlib.md の「`try_panic`」)
pub try_panic : (Unit -> <Panic | e> a) -> <e> Result String a
try_panic action =
  handle action () with
    | ctl panicked message -> Err message
    | return x -> Ok x

instance Show a => Show (List a) where
  show Nil = "[]"
  show (x :: rest) = show_rest ("[" ++ show x) rest

-- 左辺を一意な文字列として伸ばすので、長さに比例する時間で済む (docs/spec/object-model.md の「文字列の操作」)
show_rest : Show a => String -> List a -> String
show_rest acc Nil = acc ++ "]"
show_rest acc (x :: rest) = show_rest (acc ++ ", " ++ show x) rest

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

-- `lo` 以上 `hi` 未満の添字で、順に `body` を呼ぶ。`Panic` は添字の足し算から来る
-- (docs/spec/stdlib.md の「`for` と `while`」)
pub for : Int -> Int -> (Int -> <e> Unit) -> <Panic | e> Unit
for lo hi body =
  if lo < hi then
    body lo
    for (lo + 1) hi body
  else ()

-- `cond ()` が `True` を返す間、`body ()` を呼ぶ
pub while : (Unit -> <e> Bool) -> (Unit -> <e> Unit) -> <e> Unit
while cond body =
  if cond () then
    body ()
    while cond body
  else ()

-- 文字。値は Unicode のスカラー値で、順序はコードポイントの順 (`String` の UTF-8 のバイトの順と一致する)
pub extern data Char

instance Eq Char where
  extern (==)
  extern (!=)

instance Ord Char where
  extern compare
  extern (<)
  extern (<=)
  extern (>)
  extern (>=)

instance Show Char where
  extern show
  extern display

-- バイト。値は 0 から 255 の整数で、順序は数の順 (docs/spec/lir.md の「値の表現」)
pub extern data Byte

-- `+ - *` と `negate` は数学的な結果が 0 から 255 の外なら `Panic` を起こす。回すなら `Byte.wrap` で明示する
instance Num Byte where
  extern (+)
  extern (-)
  extern (*)
  extern negate

instance Div Byte where
  extern (/)
  extern (%)

instance Eq Byte where
  extern (==)
  extern (!=)

instance Ord Byte where
  extern compare
  extern (<)
  extern (<=)
  extern (>)
  extern (>=)

instance Show Byte where
  show b = display b ++ "u8"
  extern display

pub infixl 8 <<<, >>>
pub infixl 7 &&&
pub infixl 6 ^^^
pub infixl 5 |||

-- ビット演算のクラス。どのメソッドも `Panic` を起こさない。`<<<` と `>>>` の量は `Int` で、負なら逆向きにずらす
-- (docs/spec/declarations.md の「Prelude のクラス」)
pub class Bits a where
  (&&&) : a -> a -> a
  (|||) : a -> a -> a
  (^^^) : a -> a -> a
  complement : a -> a
  (<<<) : a -> Int -> a
  (>>>) : a -> Int -> a

instance Bits Int where
  extern (&&&)
  extern (|||)
  extern (^^^)
  extern complement
  extern (<<<)
  extern (>>>)

-- 結果は 0 から 255 に収まる。はみ出たビットは切り捨てる
instance Bits Byte where
  extern (&&&)
  extern (|||)
  extern (^^^)
  extern complement
  extern (<<<)
  extern (>>>)

-- 要素を持つ組み込みの型。Array の Kind は要素に従い、MutArray は常に Lin (docs/spec/types.md)
pub extern data Array a
pub extern data MutArray a

-- `var` の配列の変数の添字 `index` が長さ `len` の範囲の中か確かめ、外なら `MutArray.read` と同じ文言の `Panic` を
-- 起こす。translate だけが呼ぶ (docs/spec/core-ir.md の「`var` の変換」)
extern check_index : Int -> Int -> <Panic> Unit
