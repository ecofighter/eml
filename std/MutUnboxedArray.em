-- 要素を生の 8 バイトの語で並べ、線形に受け渡す配列 (docs/spec/stdlib.md の「`Std.MutUnboxedArray`」)。instance は
-- `Float` と `Int` だけである。`write` と `fill` はその場で書き換える。`length` と `read` は配列を借りて読み、配列を
-- 結果と組にして返す。範囲外の添字と負の長さは `Panic` である。
import Array

pub class MutUnboxed a where
  make : Int -> a -> <Panic> MutUnboxedArray a
  read : MutUnboxedArray a -> Int -> <Panic> (a, MutUnboxedArray a)
  write : MutUnboxedArray a -> Int -> a -> <Panic> MutUnboxedArray a
  fill : MutUnboxedArray a -> a -> MutUnboxedArray a

instance MutUnboxed Float where
  extern make
  extern read
  extern write
  extern fill

instance MutUnboxed Int where
  extern make
  extern read
  extern write
  extern fill

-- 長さ 0 の配列
pub extern empty : Unit -> MutUnboxedArray a
pub extern length : MutUnboxedArray a -> (Int, MutUnboxedArray a)
-- 写さずに `UnboxedArray` にする
pub extern freeze : MutUnboxedArray a -> UnboxedArray a

-- `f` を添字の順に呼ぶ。負の長さは `make` と同じ `Panic` にする
pub tabulate : MutUnboxed a => Int -> (Int -> <e> a) -> <Panic | e> MutUnboxedArray a
tabulate n f =
  let go i m =
    if i == n then m else go (i + 1) (write m i (f i))
  if n < 0 then
    -- 負の長さでは `make` に渡す要素がない。`Array.make` が同じ `Panic` を起こし、その `Unr` の結果は捨てられる
    let _ = Array.make n ()
    empty ()
  else if n == 0 then
    empty ()
  else
    let first = f 0
    go 1 (make n first)
