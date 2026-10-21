-- E1049: `data T : K` に書ける Kind は `Lin` だけで、Kind の名前でないものは誤りになる。
data Token : Linear =
  | Token

main : Unit -> <IO> Unit
main () = println "unreachable"
