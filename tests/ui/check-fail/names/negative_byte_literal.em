-- E1054: `Byte` のリテラルに負号は付けられない。式でもパターンでも同じである。
a : Byte
a = -1u8

f : Byte -> Int
f b = match b with
  | -1u8 -> 1
  | _ -> 0
