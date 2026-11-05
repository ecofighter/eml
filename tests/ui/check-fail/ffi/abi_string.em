-- E1059: ABI の文字列は `"C"` だけである。ほかの文字列を書いた宣言も、C の宣言として読む。
extern "Rust" hash : Int -> <IO> Int
extern "C++" data Widget

main : Unit -> <IO> Unit
main () = println "\{hash 1}"
