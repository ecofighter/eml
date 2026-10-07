import B

pub ping : Int -> String
ping n = if n == 0 then "done" else B.pong (n - 1)
