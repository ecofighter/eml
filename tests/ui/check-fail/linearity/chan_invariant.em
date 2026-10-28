-- E3002: `Chan` の型引数は両方向なので、`once` の関数を送った channel から受け取った値も `once` で、2回は呼べない (docs/spec/types.md の「Kind」)
import Async (Async)
import Chan
import MutArray

mk : Unit -> <IO> Chan.Chan (Unit -> <IO> Unit)
mk () = Chan.new 1

body : Unit -> <IO, Panic, Async> Unit
body () =
  let c = mk ()
  let m = MutArray.make 1 7
  let g = fn () ->
    drop m
    println "called"
  match Chan.try_send g c with
    | Chan.Accepted -> ()
    | Chan.Full x -> drop x
    | Chan.Refused x -> drop x
  match Chan.try_recv c with
    | Chan.Got h ->
      h ()
      h ()
    | Chan.Empty -> ()
    | Chan.Drained -> ()
  Chan.close c

main : Unit -> <IO, Panic> Unit
main () = Async.run body
