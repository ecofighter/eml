-- std の `List.map` と `List.filter` を繰り返し通す。どちらも逆順に積んで最後に `List.reverse` するので、手放すセルの
-- 箱を、作るセルが使い回す (docs/spec/lir.md の「reuse」)。
range : Int -> Int -> List Int -> <Panic> List Int
range lo hi acc = if hi < lo then acc else range lo (hi - 1) (hi :: acc)

step : List Int -> <Panic> List Int
step xs = List.filter (fn x -> x % 7 != 0) (List.map (fn x -> x * 3 + 1) xs)

times : Int -> List Int -> <Panic> List Int
times n xs = if n == 0 then xs else times (n - 1) (step xs)

main : Unit -> <IO, Panic> Unit
main () = println (show (List.fold (fn a x -> (a + x) % 1000000007) 0 (times 5 (range 1 20000 Nil))))
