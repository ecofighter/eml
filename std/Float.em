-- 浮動小数の関数 (docs/spec/stdlib.md の「`Std.Float`」)。値は IEEE 754 の倍精度で、計算が作る NaN はどれも正の
-- quiet NaN である。
pub extern of_int : Int -> Float
-- `Int` に収まらない結果 (NaN と ±無限大も) は `Panic` である
pub extern truncate : Float -> <Panic> Int
pub extern floor : Float -> <Panic> Int
pub extern ceil : Float -> <Panic> Int
-- ちょうど中間なら偶数の側に丸める (Python の `round` と同じ)
pub extern round : Float -> <Panic> Int
pub extern sqrt : Float -> Float
pub extern abs : Float -> Float
pub extern sin : Float -> Float
pub extern cos : Float -> Float
pub extern exp : Float -> Float
-- 自然対数
pub extern log : Float -> Float
pub extern pow : Float -> Float -> Float
-- 小数点以下 0 から 100 桁で書く (Python の `f"{x:.{n}f}"` と同じ)。桁数がその外なら `Panic` である
pub extern to_fixed : Int -> Float -> <Panic> String

pub pi : Float
pi = 3.141592653589793

-- 無限大と NaN にはリテラルがない。値の本体は `<>` なので、`Pure` の extern の関数を呼んで作る
pub infinity : Float
infinity = infinity_value ()

pub nan : Float
nan = nan_value ()

extern infinity_value : Unit -> Float
extern nan_value : Unit -> Float

-- NaN だけが自分と等しくない
pub is_nan : Float -> Bool
is_nan x = x != x

-- 前後の空白を除き、符号、10進と指数の形、`inf`、`infinity`、`nan` (大文字小文字を問わない) を読む。`_` は受けない。
-- extern は `Option` を作らないので、読めない文字列は `parse_raw` が NaN で返す。読めた結果も NaN になりうる
-- (`nan`) ので、NaN のときだけ `can_parse` で読めたかを確かめる (docs/spec/stdlib.md の「extern の境界」)
pub parse : String -> Option Float
parse s =
  let x = parse_raw s
  if x == x || can_parse s then Some x else None

extern can_parse : String -> Bool
-- 読めない文字列には NaN を返す
extern parse_raw : String -> Float
