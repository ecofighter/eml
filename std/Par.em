-- 2つの計算を組にする (docs/spec/stdlib.mdの「`Std.Par`」)。`par`は2つの子を最後まで動かして組を返し、
-- 子の`Panic`は呼んだ側で起こし直す。左の子のものが先になる。`both`は同じ組を逐次に作る関数である。

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
