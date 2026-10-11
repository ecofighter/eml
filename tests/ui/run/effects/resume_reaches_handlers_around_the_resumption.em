-- A continuation captured under one `Log` handler is resumed after that handle returns, under another `Log` handler.
-- The `ask` clause logs too. Its first run logs to the handler around the capture. After the resumption it runs as the
-- clause of the reinstalled `Ask` handler, outside that handler, so its `log` goes to the handler around the
-- resumption, as does the `log` of the resumed computation itself.
effect Ask where
  multi ask : Unit -> Int

effect Log where
  log : String -> Unit

data Saved =
  | Saved (Int -> <Log> Saved)
  | Value Int

between : Unit -> <Ask, Log> Int
between () =
  log "between"
  ask ()

capture : Unit -> <Log> Saved
capture () =
  handle Value (ask () + between ()) with
    | ask () k ->
      log "ask"
      Saved k

describe : Saved -> String
describe s =
  match s with
    | Saved _ -> "saved"
    | Value n -> show n

logging : String -> (Unit -> <Log> Saved) -> <IO> Saved
logging name body =
  handle body () with
    | log s k ->
      println (name ++ ": " ++ s)
      k ()

main : Unit -> <IO> Unit
main () =
  match logging "outer" capture with
    | Saved k ->
      let r = logging "inner" (fn () -> k 1)
      println (describe r)
      match r with
        | Saved k3 -> println (describe (logging "last" (fn () -> k3 2)))
        | Value n -> println (show n)
    | Value n -> println (show n)
