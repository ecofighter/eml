-- `Float` のフィールドを持つレコードの導出した `Eq`、`Ord`、`Show`。値を `List` に入れ、分解して作り直す。検査モード
-- でも物体を残さない。
data Point =
  | Point { label : String, x : Float, y : Float }
  deriving (Eq, Ord, Show)

shift : Float -> List Point -> <Panic> List Point
shift d ps =
  match ps with
    | [] -> []
    | Point { label, x, y } :: rest -> Point { label, x = x + d, y = y - d } :: shift d rest

main : Unit -> <IO, Panic> Unit
main () =
  let ps = [Point { label = "a", x = 1.5, y = -0.0 }, Point { label = "b", x = 0.1, y = 2.0 }]
  println (show ps)
  let moved = shift 0.25 ps
  println (show moved)
  match moved with
    | p :: q :: _ ->
      println (show (p == q))
      println (show (p == p))
      println (show (compare p q))
      println (show (p < q))
      println (show (Point { label = "c", x = -0.0, y = 0.0 } < Point { label = "c", x = 0.0, y = 0.0 }))
      println (show (Point { label = "c", x = -0.0, y = 0.0 } == Point { label = "c", x = 0.0, y = 0.0 }))
    | _ -> println "unreachable"
