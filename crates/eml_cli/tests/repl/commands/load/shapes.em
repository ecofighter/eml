import Util

data Shape = | Circle Int | Square Int

area : Shape -> <Panic> Int
area s = match s with
  | Circle r -> Util.triple (r * r)
  | Square n -> n * n

half : Int -> <Panic> Int
half n = 10 / n

same : Int -> Int
same n = match n with
  | x -> x
  | 0 -> 0
