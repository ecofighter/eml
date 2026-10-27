-- E0011: `abstract` は `pub data` と `pub effect` にだけ付く (docs/spec/grammar.md の `item`)。どの位置も `abstract` の
-- トークンに1つだけ出し、ほかの誤りは `abstract` のない宣言と同じである。
abstract data T = | T
pub abstract f : Int -> Int
f n = n
pub abstract extern data U
abstract pub data V = | V
pub abstract type W = Int

main : Unit -> <IO, Panic> Unit
main () = ()
