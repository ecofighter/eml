-- `Proc.cwd` の相対パスは `FileIn.open` と同じ基準である (docs/spec/stdlib.md の「`Proc`」)。子はこのファイルを
-- `child` の引数で流した eml で、引数のパスのファイルを、子の作業ディレクトリから読んで書く
import Env
import Proc
import Fs
import Stdout

main : Unit -> <IO, Panic> Unit
main () =
  match Env.args () with
    | ["child", path] -> Stdout.close (Stdout.write (Fs.read_text path) (Stdout.open ()))
    | _ ->
      print_capture (Proc.cwd "sub" `eml run ../main.em child hello.txt`)
      print_capture `eml run main.em child sub/hello.txt`

print_capture : Cmd -> <IO> Unit
print_capture c = println (show (Proc.capture c))
