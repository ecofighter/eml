-- E1058: 抽象型のコンストラクタは、外のモジュールの作る式、パターン、import の並びの `T(..)` に書けない
-- (docs/spec/modules.md の「公開の範囲」)。`T(..)` は型だけを取り込む。
import Lib (Handle(..))

main : Unit -> <IO, Panic> Unit
main () =
  let Lib.Handle n = Lib.make 1
  println (show (Lib.get (Lib.Handle n)))
