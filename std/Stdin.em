-- 標準入力のハンドル (docs/spec/stdlib.md の「`Stdin`」)。
import String
import Bytes

pub extern data Stdin
pub extern open : Unit -> <IO, Panic> Stdin
pub extern close : Stdin -> <IO> Unit
pub extern read_chunk : Bytes -> Stdin -> <IO> (Bytes, Stdin)
pub extern read_all_bytes : Stdin -> <IO> (Bytes, Stdin)
pub extern read_all : Stdin -> <IO> (String, Stdin)
-- 行の終わりを含む1行で、終わりなら空である
extern read_line_bytes_raw : Stdin -> <IO> (Bytes, Stdin)
extern read_line_raw : Stdin -> <IO> (String, Stdin)

pub read_text : Unit -> <IO, Panic> String
read_text () =
  let (text, h) = read_all (open ())
  close h
  text

pub read_line : Stdin -> <IO, Panic> (Option String, Stdin)
read_line h =
  let (line, h) = read_line_raw h
  if String.byte_length line == 0 then (None, h) else (Some (String.trim_line_end line), h)

pub read_line_bytes : Stdin -> <IO, Panic> (Option Bytes, Stdin)
read_line_bytes h =
  let (line, h) = read_line_bytes_raw h
  if Bytes.length line == 0 then (None, h) else (Some (Bytes.trim_line_end line), h)
