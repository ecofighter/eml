-- row の型引数を持つ `Iter` と、それを作る `generate` を、別のモジュールから使えるように公開する。
pub data Iter <e> a =
  | Done
  | More a (once Unit -> <e> Iter <e> a)

pub effect Yield a where
  yield : a -> Unit

pub generate : (Unit -> <Yield a | e> Unit) -> <e> Iter <e> a
generate producer =
  handle producer () with
    | yield x k -> More x k
    | return () -> Done
