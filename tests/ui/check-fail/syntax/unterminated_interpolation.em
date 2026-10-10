-- E0002: a hole is closed by `}` on the same line. A missing `}` is reported once at the `\{`, and the next lines
-- are read as usual. When the missing `}` leaves a `"` inside the hole, that `"` opens a new string, so the report is
-- for that string and points back at the `\{`.
broken : Int -> String
broken n = "count: \{n

typo : Int -> String
typo n = "count: \{n"

fine : Int -> String
fine n = "count: \{n}"
