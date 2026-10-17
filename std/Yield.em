-- 値を1つずつ渡す `Yield` エフェクトと、その消費者 (docs/spec/stdlib.md)。消費者は handler で包んで値を受け取る。
-- どの節も、受け取ってすぐ再開するので `fn` で書く。
import List

pub effect Yield a where
  yield : a -> Unit

-- 生産者を先に取るのは、`use x <- Yield.for_each gen` と書けるようにするため
pub for_each : (Unit -> <Yield a | e> Unit) -> (a -> <e> Unit) -> <e> Unit
for_each producer f =
  handle producer () with
    | fn yield x -> f x
    | return _ -> ()

pub fold : (b -> a -> <e> b) -> b -> (Unit -> <Yield a | e> Unit) -> <e> b
fold f init producer =
  handle producer () from init with
    | fn yield x acc -> ((), f acc x)
    | return _ acc -> acc

-- 先頭に積んでいくので、最後に逆順に戻す
pub collect : (Unit -> <Yield a | e> Unit) -> <e> List a
collect producer = List.reverse (fold (fn acc x -> x :: acc) Nil producer)
