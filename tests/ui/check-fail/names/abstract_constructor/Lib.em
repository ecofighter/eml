-- 抽象型のコンストラクタは、このモジュールの中だけで見える
pub abstract data Handle a = | Handle Int

pub make : Int -> Handle a
make n = Handle n

pub get : Handle a -> Int
get (Handle n) = n
