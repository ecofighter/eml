-- E3004, E3003 and E3006 for the state of a handler.
import FileIn

effect Ask where
  ask : Unit -> Int

effect Fail where
  never fail : Unit -> a

effect Choose where
  multi choose : Unit -> Bool

omitted : Unit -> <IO> Int
omitted () =
  handle ask () from FileIn.open "input.txt" with
    | ctl ask () k f -> k 1 f

dropped : Unit -> <IO> Int
dropped () =
  handle fail () from FileIn.open "input.txt" with
    | ctl fail () f -> 0
    | return x f ->
        FileIn.close f
        x

kept : Unit -> <Choose, IO> Int
kept () =
  handle ask () from FileIn.open "input.txt" with
    | ctl ask () k f -> k 1 f
    | return x f ->
        FileIn.close f
        x
