-- 2つの計算を組にする (docs/spec/stdlib.md の「`Std.Par`」)。`par` は2つの子の両方を待ち、子が中断すれば、
-- いちばん左の子の中断を親で起こし直す。`both` は同じ組を逐次に作る関数で、どの row の計算も受け付ける。

-- 子の row の上限は `never` なので、`IO`、`var` のエフェクト、`once` と `multi` の操作は断られる (E2020)。
-- 引数をすべて渡した呼び出しは extern の命令でなく `par` の命令になる (docs/spec/core-ir.md の「`par` の呼び出し」)
pub extern par : (Unit -> <never e> a) -> (Unit -> <never e> b) -> <e> (a, b)

-- 呼んだ側の文脈で `f`、`g` の順に呼ぶ。row に上限はない
pub both : (Unit -> <e> a) -> (Unit -> <e> b) -> <e> (a, b)
both f g =
  let a = f ()
  let b = g ()
  (a, b)
