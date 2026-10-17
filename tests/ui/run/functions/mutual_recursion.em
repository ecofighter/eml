-- `let … and …` の組で書いた相互再帰を確かめる。偶奇の判定、組の関数ごとに違う変数を捕まえる形、外で処理する
-- エフェクトを起こす形、組の中で handler を張る形を純粋な文脈から呼ぶ形、3つの関数の組、1行の形、`and` のない
-- `let` との混在、組の関数を `List.map` に渡す形 (docs/spec/expressions.md の「ローカル関数」)。
effect Ask where
  ask : Unit -> Int

parity : Int -> (Bool, Bool)
parity n =
  let even k = if k == 0 then True else odd (k - 1)
  and odd k = if k == 0 then False else even (k - 1)
  (even n, odd n)

-- `down` は `step` だけを、`up` は `limit` だけを捕まえる
bounce : Int -> Int -> Int -> List Int
bounce step limit n =
  let down k acc = if k <= 0 then List.reverse acc else up (k - step) (k :: acc)
  and up k acc = if k >= limit then List.reverse acc else down (k + 1) (k :: acc)
  down n []

asked : Int -> <Ask> Int
asked n =
  let ping k = if k == 0 then 0 else ask () + pong (k - 1)
  and pong k = if k == 0 then 0 else 10 * ask () + ping (k - 1)
  ping n

-- `f` は組の中で `Ask` を処理するので、`<>` の文脈から呼べる
handled : Int -> Int
handled m =
  let f n = handle g n with
    | fn ask () -> 7
  and g n = if n == 0 then ask () else f (n - 1)
  f m

three : Int -> String
three n =
  let a k = if k == 0 then "a" else b (k - 1)
  and b k = if k == 0 then "b" else c (k - 1)
  and c k = if k == 0 then "c" else a (k - 1)
  a n

inline : Int -> Bool
inline n = let ev k = k == 0 || od (k - 1) and od k = k != 0 && ev (k - 1) in ev n

mixed : Int -> Int
mixed n =
  let double x = 2 * x
  let collatz k steps = if k == 1 then steps else if k % 2 == 0 then halve k steps else triple k steps
  and halve k steps = collatz (k / 2) (steps + 1)
  and triple k steps = collatz (double k + k + 1) (steps + 1)
  let offset s = s + double 0
  offset (collatz n 0)

evens : List Int -> List Bool
evens xs =
  let even k = if k == 0 then True else odd (k - 1)
  and odd k = if k == 0 then False else even (k - 1)
  List.map even xs

main : Unit -> <IO> Unit
main () =
  println (show (parity 7))
  println (show (parity 100000))
  println (show (bounce 3 5 4))
  println (show (handle asked 4 with | fn ask () -> 2))
  println (show (handled 3))
  println (show (List.map three [0, 1, 2, 3, 4]))
  println (show (List.map inline [0, 1, 2, 3]))
  println (show (mixed 27))
  println (show (evens [0, 1, 2, 5, 8]))
