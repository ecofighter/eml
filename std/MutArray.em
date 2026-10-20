-- 線形に受け渡す配列 (docs/spec/stdlib.md の「`Std.MutArray`」)。`write` はその場で書き換える。`length` と `read` は
-- 配列を借りて読み、配列を結果と組にして返す。範囲外の添字と負の長さは `Panic` である。
pub extern make : Int -> a -> <Panic> MutArray a
-- 長さ 0 の配列
pub extern empty : Unit -> MutArray a
pub extern length : MutArray a -> (Int, MutArray a)
pub extern read : MutArray a -> Int -> <Panic> (a, MutArray a)
pub extern write : MutArray a -> Int -> a -> <Panic> MutArray a
-- 写さずに `Array` にする
pub extern freeze : MutArray a -> Array a

-- `f` を添字の順に呼ぶ。負の長さは `make` と同じ `Panic` にする
pub tabulate : Int -> (Int -> <e> a) -> <Panic | e> MutArray a
tabulate n f =
  let fill i m =
    if i == n then m else fill (i + 1) (write m i (f i))
  if n <= 0 then
    let _ = freeze (make n ())
    empty ()
  else
    let first = f 0
    fill 1 (make n first)
