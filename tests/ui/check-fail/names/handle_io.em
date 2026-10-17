-- E1009: the built-in `IO` cannot be handled.
quiet : Unit -> Unit
quiet () =
  handle println "hi" with
    | ctl println text k -> k ()
