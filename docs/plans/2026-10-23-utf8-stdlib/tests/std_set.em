-- `Std.Set` の関数 (docs/spec/stdlib.md の「`Std.Set`」)。集合の演算は、等しい要素のうち左の引数のものを残す
import List
import Set

-- 名前だけで比べる。`tag` は、どちらの要素が残ったかを見るためにある
data Key =
  | Key String Int

instance Eq Key where
  (Key a _) == (Key b _) = a == b

instance Ord Key where
  compare (Key a _) (Key b _) = compare a b

instance Show Key where
  show (Key name tag) = name ++ show tag

main : Unit -> <IO, Panic> Unit
main () =
  let s = Set.from_list [3, 1, 2, 1]
  println (show (Set.to_list s, Set.size s, Set.member 2 s, Set.member 9 s))
  println (show (Set.to_list (Set.insert 0 s), Set.to_list (Set.delete 2 s), Set.to_list (Set.delete 9 s)))
  println (Set.fold (fn acc x -> acc ++ show x) "" s)
  let a = Set.from_list [1, 2, 3, 4]
  let b = Set.from_list [3, 4, 5]
  println (show (Set.to_list (Set.union a b), Set.to_list (Set.intersection a b), Set.to_list (Set.difference a b)))
  println (show (Set.to_list (Set.union Set.empty b), Set.to_list (Set.intersection a Set.empty), Set.to_list (Set.difference Set.empty b)))
  -- 小さい左と大きい右、大きい左と小さい右の両方で、左の要素が残る
  let few = Set.from_list [Key "b" 1]
  let many = Set.from_list [Key "a" 2, Key "b" 2, Key "c" 2, Key "d" 2, Key "e" 2, Key "f" 2, Key "g" 2]
  println (show (Set.to_list (Set.union few many)))
  println (show (Set.to_list (Set.union many few)))
  println (show (Set.to_list (Set.intersection few many), Set.to_list (Set.intersection many few)))
  println (show (Set.to_list (Set.insert (Key "b" 3) few)))
