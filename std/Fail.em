-- 誤りを値として返す `Fail` エフェクト (docs/spec/stdlib.md)。`fail` は再開しないので `never` で、節は `ctl` で書く。

pub effect Fail x where
  never fail : x -> a

pub to_result : (Unit -> <Fail x | e> a) -> <e> Result x a
to_result action =
  handle action () with
    | ctl fail x -> Err x
    | return v -> Ok v

-- 誤りを捨てるので、誤りの型が `Unr` でなければ型検査が断る (docs/spec/types.md)
pub to_option : (Unit -> <Fail x | e> a) -> <e> Option a
to_option action =
  handle action () with
    | ctl fail _ -> None
    | return v -> Some v
