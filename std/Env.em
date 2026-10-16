-- スクリプトの引数と環境変数を読む標準モジュール (docs/spec/stdlib.md)。
extern arg_count : Unit -> <IO> Int
extern arg : Int -> <IO> String
extern has : String -> <IO> Bool
-- 設定されていなければ空の文字列である
extern var : String -> <IO> String

-- `eml` とスクリプトのパスより後ろの引数
pub args : Unit -> <IO> List String
args () = collect_args (arg_count ()) Nil

-- 後ろから読んで前に積むので、逆にしなくてよい
collect_args : Int -> List String -> <IO> List String
collect_args n acc = if n == 0 then acc else collect_args (n - 1) (arg (n - 1) :: acc)

pub get : String -> <IO> Option String
get name = if has name then Some (var name) else None
