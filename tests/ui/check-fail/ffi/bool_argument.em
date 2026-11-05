-- E2022: `Bool` は C の型に対応させない。`Int` で渡す。
extern "C" toggle : Bool -> <IO> Int

main : Unit -> <IO> Unit
main () = println "\{toggle True}"
