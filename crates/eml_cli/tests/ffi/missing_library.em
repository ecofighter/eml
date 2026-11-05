-- どの OS でも開けないライブラリを `link` に書く。`eml run` はつなげずに `link error` で終わり、`eml check` はつながない。
link on linux "eml_no_such_library"
link on macos "eml_no_such_library"
link on windows "eml_no_such_library"

extern "C" pure sqrt : Float -> Float

main : Unit -> <IO> Unit
main () = println (show (sqrt 9.0))
