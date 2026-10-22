-- deploy: 設定ファイルの段 (build、test、package、upload) を順に動かす。`deploy [--dry-run] CONFIG`。
-- 段の起動は `Run`、記録は `Log` のエフェクトにし、`--dry-run` では `Run` の handler だけを差し替える。段が失敗したら
-- そこで止め、報告を書いてから、その終了コードで終える
import Env
import Fs
import List
import Proc
import String

effect Run where
  run : String -> Result IOError Int

effect Log where
  log : String -> Unit

data Config =
  | Config { name : String, report : Option String, steps : List (String, String) }

-- 段の結果。`Broken` は段を起動できなかったことを表す
data Outcome =
  | Done
  | Failed String Int
  | Broken String

stages : List String
stages = ["build", "test", "package", "upload"]

-- 設定の1行を読む。空の行と `#` で始まる行は飛ばす
parse_line : Config -> Int -> String -> <Panic> Result String Config
parse_line config number line =
  if line == "" || String.starts_with "#" line then Ok config
  else
    match String.find "=" line with
      | None -> Err "line \{number}: expected key=value"
      | Some i ->
        let key = String.slice 0 i line
        let value = String.slice (i + 1) (String.byte_length line) line
        if key == "name" then Ok { config | name = value }
        else if key == "report" then Ok { config | report = Some value }
        else if List.length (List.filter (fn s -> s == key) stages) > 0 then
          Ok { config | steps = (key, value) :: config.steps }
        else Err "line \{number}: unknown key `\{key}`"

parse_config : String -> String -> <Panic> Result String Config
parse_config path text =
  let go config number lines =
    match lines with
      | Nil -> Ok config
      | line :: rest ->
        match parse_line config number line with
          | Ok next -> go next (number + 1) rest
          | Err message -> Err message
  go (Config { name = path, report = None, steps = Nil }) 1 (String.lines text)

-- 段 `name` のコマンド。同じ段を2回書いたら、後の行が勝つ
command_of : Config -> String -> Option String
command_of config name =
  let go steps =
    match steps with
      | Nil -> None
      | (key, value) :: rest -> if key == name then Some value else go rest
  go config.steps

-- 段を1つ動かし、成功したら残りの段 `rest` に進む
step : String -> Option String -> (Unit -> <Run, Log | e> Outcome) -> <Run, Log | e> Outcome
step name command rest =
  match command with
    | None -> rest ()
    | Some cmd ->
      log "[\{name}] run: \{cmd}"
      match run cmd with
        | Ok 0 ->
          log "[\{name}] ok"
          rest ()
        | Ok code ->
          log "[\{name}] failed with exit code \{code}"
          Failed name code
        | Err e ->
          log "[\{name}] cannot run: \{e}"
          Broken name

pipeline : Config -> <Run, Log> Outcome
pipeline config =
  use step "build" (command_of config "build")
  use step "test" (command_of config "test")
  use step "package" (command_of config "package")
  use step "upload" (command_of config "upload")
  Done

real_run : (Unit -> <Run, IO | e> a) -> <IO, Panic | e> a
real_run body =
  handle body () with
    | ctl run cmd k -> k (try_io (fn () -> Proc.run "sh" ["-c", cmd]))

dry_run : (Unit -> <Run, IO | e> a) -> <IO, Panic | e> a
dry_run body =
  handle body () with
    | ctl run _ k -> k (Ok 0)

-- 記録を標準出力に書きながら集める。集めた行は記録の順に並ぶ
collecting : (Unit -> <Log, IO | e> a) -> <IO | e> (a, List String)
collecting body =
  handle body () from Nil with
    | ctl log line k lines ->
      println line
      k () (line :: lines)
    | return x lines -> (x, List.reverse lines)

result_line : Outcome -> String
result_line outcome =
  match outcome with
    | Done -> "result: ok"
    | Failed name code -> "result: failed at \{name} (\{code})"
    | Broken name -> "result: cannot run \{name}"

read_text : String -> <IO> Result IOError String
read_text path =
  use try_io
  let f = Fs.open path
  let (text, f) = Fs.read_all f
  Fs.close f
  text

deploy : Bool -> Config -> <IO, Panic> Unit
deploy dry config =
  println "deploy \{config.name}"
  let runner = if dry then dry_run else real_run
  let (outcome, lines) =
    use collecting
    use runner
    pipeline config
  match config.report with
    | Some path ->
      let text = String.join "\n" (List.append lines [result_line outcome]) ++ "\n"
      match try_io (fn () -> Fs.write_text path text) with
        | Ok _ -> ()
        | Err e ->
          eprintln "deploy: \{e}"
          exit 2
    | None -> ()
  match outcome with
    | Done -> ()
    | Failed _ code -> exit code
    | Broken _ -> exit 2

main : Unit -> <IO, Panic> Unit
main () =
  let (dry, args) = match Env.args () with
    | "--dry-run" :: rest -> (True, rest)
    | args -> (False, args)
  match args with
    | [path] ->
      match read_text path with
        | Err e ->
          eprintln "deploy: \{e}"
          exit 2
        | Ok text ->
          match parse_config path text with
            | Ok config -> deploy dry config
            | Err message ->
              eprintln "deploy: \{message}"
              exit 2
    | _ ->
      eprintln "usage: deploy [--dry-run] CONFIG"
      exit 2
