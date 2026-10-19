-- 子のプロセスを起動する標準モジュール (docs/spec/stdlib.md)。
import List

-- プログラムと引数の列。列は、引数の数と各引数を、それぞれ NUL で終えて並べたものである。OS の引数は NUL を含め
-- ないので、NUL を含む引数があれば数が合わず、起動の誤りになる
extern status : String -> String -> <IO> Int

-- `PATH` から `program` を探して起動し、終わるのを待って終了コードを返す。子はシェルを介さず、標準入出力と環境を
-- 受け継ぐ
pub run : String -> List String -> <IO, Panic> Int
run program args = status program (terminated args ("\{List.length args}\0"))

terminated : List String -> String -> String
terminated Nil acc = acc
terminated (a :: rest) acc = terminated rest ((acc ++ a) ++ "\0")
