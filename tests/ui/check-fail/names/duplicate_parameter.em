-- E1003: 型引数の名前は、row か型かによらず1つの宣言の中で重ねられない。
data Pair <e> e =
  | Pair Int

main : Unit -> <IO> Unit
main () = println "unreachable"
