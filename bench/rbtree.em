-- 赤黒木への挿入。`balance` が組み直す節を受ける形 (FBIP の形) に書いたので、`ins` が分解した節の箱を、同じ関数の中で
-- 作る節が使い回す。Okasaki の書き方 (`balance c l k r`) では節を作るのが呼び出し先なので、関数をまたぐ reuse が要る
-- (docs/implementation/status.md の「深さと性能」)。
data Color =
  | Red
  | Black

data Tree =
  | Leaf
  | Node Color Tree Int Tree

balance : Tree -> Tree
balance (Node Black (Node Red (Node Red a x b) y c) z d) = Node Red (Node Black a x b) y (Node Black c z d)
balance (Node Black (Node Red a x (Node Red b y c)) z d) = Node Red (Node Black a x b) y (Node Black c z d)
balance (Node Black a x (Node Red (Node Red b y c) z d)) = Node Red (Node Black a x b) y (Node Black c z d)
balance (Node Black a x (Node Red b y (Node Red c z d))) = Node Red (Node Black a x b) y (Node Black c z d)
balance t = t

ins : Int -> Tree -> Tree
ins x t = match t with
  | Leaf -> Node Red Leaf x Leaf
  | Node c l k r ->
    if x < k then balance (Node c (ins x l) k r)
    else if x > k then balance (Node c l k (ins x r))
    else Node c l k r

blacken : Tree -> Tree
blacken t = match t with
  | Node Red l k r -> Node Black l k r
  | _ -> t

-- 線形合同法で次の種を作り、種の下位の桁を木に入れる
build : Int -> Int -> Tree -> <Panic> Tree
build n seed t = if n == 0 then t else
  let next = (seed * 1103515245 + 12345) % 2147483648
  build (n - 1) next (blacken (ins (next % 10000000) t))

count : Tree -> <Panic> Int
count t = match t with
  | Leaf -> 0
  | Node _ l _ r -> count l + 1 + count r

main : Unit -> <IO, Panic> Unit
main () = println (show (count (build 10000 42 Leaf)))
