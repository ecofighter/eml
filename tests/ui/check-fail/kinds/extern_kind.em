-- E0011: extern の型の Kind は処理系の表が決めるので、`extern data` の後ろに `: Lin` は書けない。
extern data Handle : Lin

main : Unit -> <IO> Unit
main () = println "unreachable"
