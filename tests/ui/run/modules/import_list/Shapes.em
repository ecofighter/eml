pub data Shape =
  | Square Int
  | Rect Int Int

pub area : Shape -> Int
area s = match s with
  | Square n -> n * n
  | Rect w h -> w * h
