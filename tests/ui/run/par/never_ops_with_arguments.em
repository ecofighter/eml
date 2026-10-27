-- 子の `never` の操作の引数は、起こし直す閉包が捕まえて親へ運ぶ (docs/spec/core-ir.md の「`par` の子の中断」)。
-- 引数は `Int`、`Unit`、`Float`、`String` と `Lin` の `MutArray` で、1つから3つまでである。左の子の中断が勝つと、
-- 右の子の閉包は `MutArray` ごと捨てる。
import Std.Par
import Std.MutArray
import Std.Array

effect Stop where
  never stop_int : Int -> a
  never stop_unit : Unit -> a
  never stop_two : Int -> String -> a
  never stop_three : Int -> Float -> String -> a
  never stop_array : Float -> MutArray Int -> a

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

catch : (Unit -> <Stop, Panic | e> (Int, Int)) -> <Panic | e> String
catch action =
  handle action () with
    | ctl stop_int n -> "int " ++ show n
    | ctl stop_unit u -> "unit " ++ show u
    | ctl stop_two n s -> "two " ++ show n ++ " " ++ s
    | ctl stop_three n x s -> "three " ++ show n ++ " " ++ show x ++ " " ++ s
    | ctl stop_array x m -> "array " ++ show x ++ " " ++ show (Array.length (MutArray.freeze m))
    | return (a, b) -> "values " ++ show (a + b)

main : Unit -> <IO, Panic> Unit
main () =
  println (catch (fn () -> Par.par (fn () -> spin 40 + stop_int 1) (fn () -> 2)))
  println (catch (fn () -> Par.par (fn () -> spin 40) (fn () -> stop_unit ())))
  println (catch (fn () -> Par.par (fn () -> spin 40) (fn () -> stop_two 2 "b")))
  println (catch (fn () -> Par.par (fn () -> spin 40 + stop_three 3 0.5 "c") (fn () -> 3)))
  println (catch (fn () -> Par.par (fn () -> spin 40) (fn () -> stop_array 2.5 (MutArray.make 3 0))))
  println (catch (fn () -> Par.par (fn () -> spin 40 + stop_array 1.25 (MutArray.make 4 0)) (fn () -> stop_array 0.75 (MutArray.make 6 0))))
