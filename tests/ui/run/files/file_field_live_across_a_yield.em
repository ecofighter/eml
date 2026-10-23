-- `data` から借りた `FileIn` が yield をまたいで生きている。持ち主 `p` は分解の後に使われないので、Perceus は節の呼び出しの前に
-- `p` を `release` で解き、`f` を `dup` せずに受け取る。続きのクロージャは `f` を捕まえるので `lin` になり、2 回目の handle では
-- `drop k` が `f` を閉じる。
import FileIn

effect Ask where
  ask : Unit -> Int

data P =
  | P FileIn.FileIn String

body : P -> <Ask, IO> Unit
body p = match p with
  | P f s ->
      let n = ask ()
      FileIn.close f
      println (s ++ show n)

main : Unit -> <IO> Unit
main () =
  handle body (P (FileIn.open "input.txt") "tag") with
    | ctl ask () k -> k 7
  handle body (P (FileIn.open "input.txt") "dropped") with
    | ctl ask () k -> drop k
