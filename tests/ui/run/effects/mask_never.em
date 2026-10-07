-- `never` の操作が `Mask` フレームを越えて中断する。区間の `Mask` フレームも解放される (debug_heap)。
effect Fail where
  never fail : String -> a

effect Log where
  log : String -> Unit

quiet : (Unit -> <e> a) -> <e> a
quiet action =
  handle run action with
    | log _ k -> k ()

run : (Unit -> <e> a) -> <Log | e> a
run action =
  log "start"
  action ()

boom : Unit -> <Fail, Log> String
boom () =
  log "boom"
  fail "stop"

main : Unit -> <IO> Unit
main () =
  let r = handle (handle quiet boom with
                    | log m k ->
                        println m
                        k ()) with
            | fail msg -> msg
  println r
