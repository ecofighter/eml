-- E1045, E1046 and E1047: a field declared twice, a field the constructor does not have, and a missing field.
data P = | P { name : String, name : Int }

data Q = | Q { x : Int, y : Int }

f : Unit -> Q
f () = Q { x = 1, z = 2 }

g : Unit -> Q
g () = Q { y = 1 }

main : Unit -> <IO> Unit
main () = ()
