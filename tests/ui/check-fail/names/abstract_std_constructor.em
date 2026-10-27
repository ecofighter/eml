-- E1058: `Std.Map` の `Map` は抽象型で、コンストラクタを外のモジュールのパターンに書けない。診断は正式な名前
-- `Std.Map` を書く (docs/spec/modules.md の「公開の範囲」)。
import Map

main : Unit -> <IO> Unit
main () =
  match Map.insert 1 2 Map.empty with
    | Map.Empty -> println "empty"
    | _ -> println "node"
