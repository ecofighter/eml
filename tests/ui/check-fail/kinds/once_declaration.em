-- E1050: トップレベルの関数、操作、クラスのメソッドのシグネチャの一番外側の矢印は、宣言したものそのものを表し、
-- 何度でも呼べる。そこに `once` を書くと誤りになる。内側の矢印の `once` は書ける。
effect Ask where
  ask : once Unit -> Int

class Run a where
  run : once a -> Int

inc : once Int -> <Panic> Int
inc x = x + 1

apply_once : (once Int -> Int) -> Int
apply_once f = f 1

main : Unit -> <IO> Unit
main () = println "unreachable"
