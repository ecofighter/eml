-- windows: skip (子のプロセスに POSIX のツールを使う)
-- `Proc.cwd` の相対パスは `FileIn.open` と同じ基準で、`/` を含む相対のプログラムは子の作業ディレクトリに対して
-- 解決する (docs/spec/stdlib.md の「`Proc`」)
import Proc

main : Unit -> <IO, Panic> Unit
main () =
  print_capture (Proc.cwd "sub" `cat hello.txt`)
  print_capture (Proc.cwd "sub" `./run.sh`)
  print_capture `cat sub/hello.txt`

print_capture : Cmd -> <IO> Unit
print_capture c = println (show (Proc.capture c))
