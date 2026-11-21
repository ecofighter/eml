-- 2つの計算を組にする (docs/spec/stdlib.mdの「`Std.Par`」)。`par`は2つの子を最後まで動かして組を返し、
-- 子の`Panic`は呼んだ側で起こし直す。左の子のものが先になる。`both`は同じ組を逐次に作る関数である。
import Array
import MutArray
import MutUnboxedArray
import UnboxedArray (Unboxed)

-- forkとjoin。子は全域で、子から出るのは値だけである。引数をすべて渡した呼び出しは
-- externの命令でなく`par`の命令になる (docs/spec/core-ir.mdの「`par`の呼び出し」)
extern fork_join : (Unit -> a) -> (Unit -> b) -> (a, b)

-- 子はそれぞれ`try_panic`の下で動く。失敗した組は相手の側を名前で束縛するので、`Lin`の結果は
-- `panic`の前に捨てられる (docs/spec/linearity.mdの「暗黙の後始末」)
pub par : (Unit -> <Panic> a) -> (Unit -> <Panic> b) -> <Panic> (a, b)
par f g =
  match fork_join (fn () -> try_panic f) (fn () -> try_panic g) with
    | (Ok a, Ok b) -> (a, b)
    | (Err m, r) -> panic m
    | (l, Err m) -> panic m

-- 呼んだ側の文脈で `f`、`g` の順に呼ぶ。row に上限はない
pub both : (Unit -> <e> a) -> (Unit -> <e> b) -> <e> (a, b)
both f g =
  let a = f ()
  let b = g ()
  (a, b)

-- `_grain`のない関数で、塊に入る添字の数 (docs/spec/stdlib.mdの「`Std.Par`」)
grain : Int
grain = 1024

-- `b`から数え上げて`b * b >= n`となる最初の整数
root : Int -> Int -> <Panic> Int
root n b = if b * b >= n then b else root n (b + 1)

-- `lo`から`hi`の手前までの空でない範囲を、2段の背骨に分ける。範囲を`size`個の添字ごとの塊に切り、
-- 塊を`width`個ずつの群に分ける。`width`は塊の数の平方根を切り上げた値である。上の段は、それより前の
-- すべての群と最後の群を`par`に渡し、群は、それより前のすべての塊と最後の塊を`par`に渡す。
-- `leaf`は境界から塊を計算し、`join`は隣り合う2つを左から順に結ぶので、どちらの段でも結合は左に入れ子になる。
-- 長さ`hi - lo`が`Int`に収まらない範囲では、この引き算が算術の`Panic` (`integer overflow`)を起こす。
-- 形は長さと`size`だけで決まり、workerの数にも、別のworkerがどちらの子を取ったかにも依らない
spine : Int -> Int -> Int -> (Int -> Int -> <Panic> a) -> (a -> a -> <Panic> a) -> <Panic> a
spine size lo hi leaf join =
  let chunks = if size < 1 then panic "Par: the grain is less than 1" else (hi - lo - 1) / size + 1
  let width = root chunks 1
  let chunk k = leaf (lo + k * size) (if k + 1 == chunks then hi else lo + (k + 1) * size)
  let group first last =
    if first == last then
      chunk first
    else
      let (rest, tail) = par (fn () -> group first (last - 1)) (fn () -> chunk last)
      join rest tail
  let top g =
    let first = g * width
    let last = if first + width < chunks then first + width - 1 else chunks - 1
    if g == 0 then
      group first last
    else
      let (rest, tail) = par (fn () -> top (g - 1)) (fn () -> group first last)
      join rest tail
  top ((chunks - 1) / width)

-- `lo`から`hi`の手前までの添字の`f i`を、背骨の順に結ぶ。塊は最初の添字から始めて添字を左から畳む。
-- 空の範囲は`unit`を返し、`unit`はほかでは使わない
pub reduce_range_grain : Int -> (a -> a -> <Panic> a) -> a -> Int -> Int -> (Int -> <Panic> a) -> <Panic> a
reduce_range_grain size join unit lo hi f =
  let fold i end acc = if i == end then acc else fold (i + 1) end (join acc (f i))
  if lo < hi then spine size lo hi (fn a b -> fold (a + 1) b (f a)) join else unit

pub reduce_range : (a -> a -> <Panic> a) -> a -> Int -> Int -> (Int -> <Panic> a) -> <Panic> a
reduce_range join unit lo hi f = reduce_range_grain grain join unit lo hi f

-- `lo`から`hi`の手前までの添字について`body i`を呼ぶ。塊の中は添字の順で、塊の間の順は決まっていない
pub for_range_grain : Int -> Int -> Int -> (Int -> <Panic> Unit) -> <Panic> Unit
for_range_grain size lo hi body =
  if lo < hi then spine size lo hi (fn a b -> for a b body) (fn () () -> ()) else ()

pub for_range : Int -> Int -> (Int -> <Panic> Unit) -> <Panic> Unit
for_range lo hi body = for_range_grain grain lo hi body

-- 背骨の`join`が組み立てる、塊の結果の木。`flatten`は塊を左から右の順に並べる
data Rope c =
  | Leaf c
  | Join (Rope c) (Rope c)

flatten : Rope c -> Array c
flatten rope =
  let collect r acc =
    match r with
      | Leaf c -> c :: acc
      | Join l r -> collect l (collect r acc)
  Array.from_list (collect rope [])

-- 要素`i`が`f i`である配列。塊は逐次の`tabulate`で作り、塊は`Array.concat`で結ぶ。粒度以下の長さは、
-- 負の長さも含めて、逐次の`tabulate`そのものである
pub tabulate : Int -> (Int -> <Panic> a) -> <Panic> Array a
tabulate n f =
  if n <= grain then
    Array.tabulate n f
  else
    Array.concat (flatten (spine grain 0 n (fn a b -> Leaf (Array.tabulate (b - a) (fn i -> f (a + i)))) Join))

pub tabulate_unboxed : Unboxed a => Int -> (Int -> <Panic> a) -> <Panic> UnboxedArray a
tabulate_unboxed n f =
  if n <= grain then
    UnboxedArray.tabulate n f
  else
    UnboxedArray.concat
      (flatten (spine grain 0 n (fn a b -> Leaf (UnboxedArray.tabulate (b - a) (fn i -> f (a + i)))) Join))

pub mapi : (Int -> a -> <Panic> b) -> Array a -> <Panic> Array b
mapi f xs = tabulate (Array.length xs) (fn i -> f i (Array.get xs i))

pub map : (a -> <Panic> b) -> Array a -> <Panic> Array b
map f xs = tabulate (Array.length xs) (fn i -> f (Array.get xs i))

-- 要素を背骨の順に結ぶ。空の配列は`unit`を返す
pub reduce : (a -> a -> <Panic> a) -> a -> Array a -> <Panic> a
reduce join unit xs = reduce_range join unit 0 (Array.length xs) (fn i -> Array.get xs i)

-- `Result`の配列を、最も左の`Err`か、値の配列にする
sequence : Array (Result x b) -> <Panic> Result x (Array b)
sequence rs =
  let n = Array.length rs
  let first i =
    if i == n then
      None
    else
      match Array.get rs i with
        | Err e -> Some e
        | Ok _ -> first (i + 1)
  let value i =
    match Array.get rs i with
      | Ok y -> y
      | Err _ -> panic "Par: an `Err` is left after the scan"
  match first 0 with
    | Some e -> Err e
    | None -> Ok (Array.tabulate n value)

-- すべての要素に`f`を呼んだ後で、`Err`があれば最も左のものを返す。塊は、`sequence`が`Err`を探す前に
-- すべての要素を評価し、`join`は左の`Err`を先に取る
pub try_map : (a -> <Panic> Result x b) -> Array a -> <Panic> Result x (Array b)
try_map f xs =
  let n = Array.length xs
  let chunk a b = sequence (Array.tabulate (b - a) (fn i -> f (Array.get xs (a + i))))
  let leaf a b =
    match chunk a b with
      | Ok ys -> Ok (Leaf ys)
      | Err e -> Err e
  let join l r =
    match (l, r) with
      | (Ok x, Ok y) -> Ok (Join x y)
      | (Err e, _) -> Err e
      | (_, Err e) -> Err e
  if n <= grain then
    chunk 0 n
  else
    match spine grain 0 n leaf join with
      | Ok rope -> Ok (Array.concat (flatten rope))
      | Err e -> Err e

-- 配列を凍結して`mapi`を呼び、結果を解凍する。結果は一意なので、コピーは起こらない
pub for_mut : (Int -> a -> <Panic> a) -> MutArray a -> <Panic> MutArray a
for_mut f m = Array.thaw (mapi f (MutArray.freeze m))

-- `for_mut`と同じく、凍結した入力から新しい配列を作って解凍するので、`thaw`はコピーしない
pub for_mut_unboxed : Unboxed a => (Int -> a -> <Panic> a) -> MutUnboxedArray a -> <Panic> MutUnboxedArray a
for_mut_unboxed f m =
  let xs = MutUnboxedArray.freeze m
  UnboxedArray.thaw (tabulate_unboxed (UnboxedArray.length xs) (fn i -> f i (UnboxedArray.get xs i)))
