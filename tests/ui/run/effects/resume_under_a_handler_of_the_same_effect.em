-- 節の中で同じエフェクトの handler を張り、その中で `k 1` を呼ぶ。再開した本体の `return` の節の `op ()` は、
-- 内側の handler (100) に届く。row が推論用の末尾のラムダ (`inferred`) でも、rigid な末尾のシグネチャ (`rigid`) でも
-- 同じ handler に届き、どちらも 101 を出す (docs/spec/types.md の「呼び出しの包含を解く」)。
effect L where
  op : Unit -> Int

rigid : (Unit -> <e> Int) -> <L, Panic | e> Int
rigid h =
  handle op () with
    | ctl op () k ->
        handle k 1 with
          | fn op () -> 100
    | return x -> x + op ()

main : Unit -> <IO, Panic> Unit
main () =
  let inferred = fn () ->
    handle op () with
      | ctl op () k ->
          handle k 1 with
            | fn op () -> 100
      | return x -> x + op ()
  let a = handle inferred () with
    | fn op () -> 1000
  let b = handle rigid (fn () -> 0) with
    | fn op () -> 1000
  println (show a)
  println (show b)
