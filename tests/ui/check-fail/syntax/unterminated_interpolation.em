-- E0002: a hole is closed by `}` on the same line. A missing `}` is reported once at the `\{`, and the next lines
-- are read as usual.
broken : Int -> String
broken n = "count: \{n"

fine : Int -> String
fine n = "count: \{n}"
