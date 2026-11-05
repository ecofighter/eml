-- E0004: `blocking` の C の関数はまだ呼べない。行が正しい宣言にだけ出す。
import Async (Async)

extern "C" blocking sleep : Int as size_t -> <IO, Panic, Async> Int

main : Unit -> <IO> Unit
main () = println "x"
