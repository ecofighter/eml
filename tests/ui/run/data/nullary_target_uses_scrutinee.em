-- フィールドのない枝 (`None`) が、scrutinee を別の関数に渡してもう一度使う。漏れも二重の解放もない。
data Opt =
  | None
  | Some Int

describe : Opt -> Int
describe o = match o with
  | None -> weight o
  | Some n -> n

weight : Opt -> Int
weight o = match o with
  | None -> 0
  | Some n -> n + 1

main : Unit -> <IO> Unit
main () = println (show_int (describe None + describe (Some 3)))
