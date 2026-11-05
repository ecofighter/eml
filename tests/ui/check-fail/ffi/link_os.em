-- E1060: `link` の OS は `linux`、`macos`、`windows` のどれかで、`framework` は `macos` にだけ書ける。名前は空にできない。
link on plan9 "libm.so"
link on linux framework "Accelerate"
link on macos framework "Accelerate"
link on macos framework ""

main : Unit -> <IO> Unit
main () = println "x"
