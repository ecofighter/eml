-- E3004, E3003 and E3006 for the state of a handler.
effect Ask where
  ask : Unit -> Int

effect Fail where
  never fail : Unit -> a

effect Choose where
  multi choose : Unit -> Bool

omitted : Unit -> <IO> Int
omitted () =
  handle ask () from Fs.open "input.txt" with
    | ask () k f -> k 1 f

dropped : Unit -> <IO> Int
dropped () =
  handle fail () from Fs.open "input.txt" with
    | fail () f -> 0
    | return x f ->
        Fs.close f
        x

kept : Unit -> <Choose, IO> Int
kept () =
  handle ask () from Fs.open "input.txt" with
    | ask () k f -> k 1 f
    | return x f ->
        Fs.close f
        x
