-- 抽象エフェクトの操作は、このモジュールの中だけで perform と handle ができる
pub abstract effect Ping where
  ping : Unit -> Int

pub twice : Unit -> <Ping, Panic> Int
twice () = ping () + ping ()

pub answer : (Unit -> <Ping, Panic> Int) -> <Panic> Int
answer f =
  handle f () with
    | ctl ping () k -> k 21
    | return x -> x
