-- E1055: a spread inside a quote, or next to an empty quote, is not a bare argument.
f : List String -> Cmd
f xs = `ls '\{..xs}' ''\{..xs}`
