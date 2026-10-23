-- E1055: a `\{..xs}` hole spreads only as a whole argument. Next to text or another hole it is reported, once per hole.
f : List String -> Cmd
f xs = `cp --files=\{..xs} \{..xs}\{..xs} dest`
