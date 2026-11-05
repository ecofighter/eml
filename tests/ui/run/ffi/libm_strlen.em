-- `Bytes` の引数は本体へのポインタだけを渡す。C の文字列として読ませるので、終わりに 0 のバイトを置く。
link on linux "libm.so.6"
link on macos "m"
link on windows "ucrtbase"

import Bytes

extern "C" pure strlen : Bytes -> Int as size_t

main : Unit -> <IO> Unit
main () = println (show (strlen (Bytes.of_string "abc\0")))
