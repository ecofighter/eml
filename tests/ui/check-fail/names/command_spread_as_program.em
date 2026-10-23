-- E1055: the first word is the program, so it cannot be a spread.
f : List String -> Cmd
f xs = `\{..xs} -l`
