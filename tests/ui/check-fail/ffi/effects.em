-- E2023: 最後の矢印の行は、`pure` でなければ `IO`、狭める `Int as` の引数かハンドルの結果があれば `Panic` をちょうど1つずつ持つ。
extern "C" data Solver

extern "C" pure cos : Float -> <IO> Float
extern "C" abs : Int as int -> <IO> Int
extern "C" pure open : Int -> Solver
extern "C" add : Int -> <IO> Int -> Int
extern "C" twice : Int -> <IO, IO> Int
extern "C" make : Int as int -> <IO> Solver

main : Unit -> <IO> Unit
main () = println "\{cos 0.0}"
