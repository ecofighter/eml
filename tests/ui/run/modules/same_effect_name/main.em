-- `Log` is declared both here and in `Audit`. The two effects stay apart: each handler handles only the operations
-- of its own effect.
import Audit

effect Log where
  emit : Int -> Unit

local : Unit -> <Log> Unit
local () = emit 2

main : Unit -> <IO> Unit
main () =
  handle Audit.audited () with
    | Audit.emit n k -> resume k (println ("audit " ++ show_int n))
  handle local () with
    | emit n k -> resume k (println ("local " ++ show_int n))
