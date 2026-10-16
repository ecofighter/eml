import Util

data Shape = | Circle Int | Square Int

area : Shape -> Int
area s = match s with
  | Circle r -> Util.triple (r * r)
  | Square n -> n * n

half : Int -> Int
half n = 10 / n
