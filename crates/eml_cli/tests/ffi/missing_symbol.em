-- `link` がないので記号はプロセスの中だけで探し、見つからなければ `link error` で終わる。
extern "C" pure symbol "eml_no_such_symbol" missing : Float -> Float

main : Unit -> <IO> Unit
main () = println (show (missing 9.0))
