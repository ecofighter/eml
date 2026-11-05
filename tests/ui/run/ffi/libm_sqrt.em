-- C の標準ライブラリの `sqrt` を `link` で開いて呼ぶ。`pure` の関数は効果を持たない。
link on linux "libm.so.6"
link on macos "m"
link on windows "ucrtbase"

import Float

extern "C" pure sqrt : Float -> Float

main : Unit -> <IO, Panic> Unit
main () = println (show (Float.truncate (sqrt 9.0)))
