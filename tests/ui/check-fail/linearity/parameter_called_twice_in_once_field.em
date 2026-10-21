-- E3002: 2回呼んだ引数を `once` のフィールドに入れると、矢印の Kind 変数が `Lin` と `Unr` の両方に縛られ、使用回数の
-- 誤りになる。ラムダで包めば入れられる (`Once (fn x -> f x)`)。
data Once =
  | Once (once Int -> Int)

keep : (Int -> Int) -> (Int, Once)
keep f = (f 1, Once f)

both : (Int -> Int) -> Once
both f =
  let n = f 0
  Once f

main : Unit -> <IO> Unit
main () = println "unreachable"
