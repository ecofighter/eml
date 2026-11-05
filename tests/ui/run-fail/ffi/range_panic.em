-- 範囲の外の `Int as int` の引数で上げた `Panic` を捕まえないと、その文言と呼び出しの位置を持つ実行時エラーになる。
link on linux "libm.so.6"
link on macos "m"
link on windows "ucrtbase"

extern "C" pure abs : Int as int -> <Panic> Int as int

main : Unit -> <IO, Panic> Unit
main () =
  println "before"
  println (show (abs 3000000000))
