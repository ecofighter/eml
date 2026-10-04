-- E1006: comparisons do not associate.
main : Unit -> <IO> Unit
main () = if 1 < 2 < 3 then println "yes"
