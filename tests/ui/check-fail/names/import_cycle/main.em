-- E1027: `A` imports `B` and `B` imports `A`. Checking goes on after the cycle, so nothing else is reported.
import A

main : Unit -> <IO> Unit
main () = println (A.ping 2)
