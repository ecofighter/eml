-- E2001: a hole takes only a `String`; it does not call `display`. A list goes in with `\{..xs}`.
f : List String -> Cmd
f xs = `echo \{1} \{xs}`
