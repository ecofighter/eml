-- E2023: `blocking` の行は `IO` と `Async` をいつも持ち、狭める `Int as` の引数かハンドルの結果があれば `Panic` も持つ。
import Async (Async)

extern "C" data Solver

extern "C" blocking wait : Int -> <IO> Int
extern "C" blocking sleep : Int as size_t -> <IO, Async> Int
extern "C" blocking open : Int -> <IO, Panic, Async> Solver
extern "C" blocking tick : Int -> <IO, Panic, Async> Int

main : Unit -> <IO> Unit
main () = println "x"
