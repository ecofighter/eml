-- E2022: C の関数の型は位置ごとに1つに決まるので、型変数は書けない。
extern "C" identity : a -> <IO> a

main : Unit -> <IO> Unit
main () = println "\{identity 1}"
