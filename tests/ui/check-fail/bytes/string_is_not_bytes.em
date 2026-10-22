-- E2001: `String` と `Bytes` は別の型で、暗黙には変換できない。
size : Bytes -> Int
size b = Bytes.length b

total : Int
total = size "abc"
