-- タスクの間で共有する数 (docs/spec/stdlib.md の「`Std.Atomic`」)。`Counter` は `Machine` が run をまたいで持つ表の
-- 添字を持つ `Unr` の即値で、放さない。数の少ない共有の状態のためのものである
-- `Counter` の行は登録表を使う `Async` の命令で動くので、`Async` を読み込んでエントリの関数を置く
-- (docs/spec/vm.md の「`Async` のタスク」)
import Async

pub extern data Counter

-- 初めの値 `n` の数を作る
pub extern new   : Int -> <IO> Counter
-- `n` を足し、足した後の値を返す。64 ビットで巻き戻る
pub extern add   : Int -> Counter -> <IO> Int
pub extern load  : Counter -> <IO> Int
pub extern store : Int -> Counter -> <IO> Unit
