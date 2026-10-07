-- A module deep under the root is named by its whole path and qualified by its last segment. Its own imports are
-- read from the root too, not from its directory.
import Deep.Inner.Most.Leaf

main : Unit -> <IO> Unit
main () = println (Leaf.leaf "x")
