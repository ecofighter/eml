-- E0018: `extern "C"` の属性はどれも1回だけ書け、`pure` と `blocking` は一緒に書けない。ABI のない `extern` にも書けない。
extern "C" pure pure sin : Float -> Float
extern "C" pure blocking cos : Float -> <IO> Float
extern pure tan : Float -> Float

main : Unit -> <IO> Unit
main () = println "\{sin 0.0}"
