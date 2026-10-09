# テスト戦略

位置づけ: 手引き。

テストの書き方、テストの変更の運用、テストの置き場所、層ごとの方法、Core IR のテキストの形、UI テストの仕組みを定める。

## 方針

- TDD で進める。テストを先に書き、実装をテストに合わせる
- 既存のテストは合意済みの仕様である。テストが失敗したら実装を直す。テストを変えるときは、下の「テストの変更の運用」に従う
- スナップショットは `insta` を使う。多くはインラインのスナップショット (`@"..."`) にする
- スナップショットの更新は `cargo insta review` で行う

## テストの変更の運用

テストの変更を3種類に分け、種類ごとに合意の取り方を決める。このプロジェクトで合意した運用で、「既存のテストを変えない」という原則の例外にあたる。

| 種類 | 何が変わるか | 合意と記録 |
|---|---|---|
| 成否の変更 | `run` / `run-fail` / `check-fail` の間でのテストの移動と、テストの削除 (UI テストと単体テスト) | 作業の spec に1件ずつ挙げる。spec の承認を合意とみなす |
| 期待値の変更 | UI テストの出力、診断の番号と文言、各段階のダンプのスナップショット、成否を変えない UI テストの移動 | 作業の spec に、変わるテストを範囲で書く (「E3005 を使う UI テストはすべて書き換える」「Core IR のダンプはすべて取り直す」など)。spec の承認を、その範囲の変更すべてへの合意とみなす |
| 機械的な追随 | テストの組み立てだけが変わり、スナップショットの文字列と `assert` の値は1文字も変わらない | 合意は要らない。記録はコミットメッセージで足りる |

- テストを変えないことを理由に設計を曲げない。テストの変更が要ると分かったら、上のどの種類に当たるかを示して、作業の spec に書く
- 成否の変更と期待値の変更では、変える理由を作業の spec とコミットメッセージに書く
- 作業の計画の全体制約は「成否と期待値は、spec に挙げたテストと範囲の中だけで変える。期待値を変えない機械的な追随は許す」と書く
- UI テストの最上位のディレクトリが成否を決める

## テストの置き場所

新しいテストの置き場所は、期待するものを上から順に見て、最初に当てはまる行で決める。

| # | 期待するもの | 置き場所 |
|---|---|---|
| 1 | 機能ごとの代表的なプログラムを `eml check` / `eml run` にかけたときにユーザーが見るもの。stdout、実行時エラー、プログラム全体が通るか落ちるか、代表的な診断の表示 | UI テスト (`tests/ui/`) |
| 2 | 1つの段階の出力 (CST、HIR、推論したシグネチャ、Core IR) と、その段階の診断の細部 (位置、回復、端のケース) | その段階の crate の結合テスト (`crates/<段階>/tests/`)。ソースから `eml_test_support` で組み立てる |
| 3 | フロントエンドからは作れない状態 (壊れた IR、`decref` の抜けた IR など) と、テストの中で生成した大きなソース | `eml_core_ir/tests/verify.rs` か `eml_interp/tests/`。Core IR は IR のテキストで書き、`eml_core_ir::parse` で読む |
| 4 | 公開の API から届かないか、内部の状態を直接組まないと確かめにくい部品の振る舞い。レイアウト段、パーサのマーカー、単一化の表、Kind の制約の解消、ヒープなど | `src/` の単体テスト |
| 5 | lib API の流れ、CLI の終了コード、テスト補助そのもの | `eml_cli/tests/api.rs`、`eml_cli/tests/cli.rs`、`eml_test_support/tests/` |

### 重複させない

1つの事実は、上の表で選んだ1か所だけで確かめる。UI テストは機能ごとの代表的な筋書きを確かめ、段階の端のケースを繰り返さない。例えば診断なら、端のケースは段階の crate のテストに置き、代表的な表示を `check-fail/` に1つ置く。

### crate の中の置き方

- 結合テストは、話題ごとに1ファイルにする。ファイル名は spec の節か言語の機能から付ける (`effects.rs`、`operators.rs`)。1つのファイルが複数の話題にまたがったら、話題で分ける
- 結合テストは、crate ごとに1つのバイナリ (`integration`) にまとめる。`Cargo.toml` に `autotests = false` と `[[test]]` を書き、`tests/main.rs` で各ファイルを `mod` で宣言する。テストのバイナリが増えると、リンクと、macOS が新しい実行ファイルを最初に起動するときの検査に時間がかかるためである。`tests/main.rs` で宣言しないファイルはコンパイルされず、テストが流れない
- lib には `doctest = false` を付ける。doc コメントは説明だけで、実行する例を書かない。単体テストのない lib (`eml_extern`、`eml_hir`、`eml_core_ir`、`eml_cli`、`eml_test_support`) と `eml_cli` の bin には `test = false` も付け、空のテストのバイナリを作らない。単体テストを足すときは `test = false` を外す
- crate の結合テストが使う表示の関数は `tests/common/mod.rs` に置き、`tests/main.rs` で1回だけ宣言して、各ファイルから `crate::common` で使う。複数の crate で使う部品は `eml_test_support` に置く
- Core IR の結合テストは、確かめるパスごとのファイル (`translate.rs`、`boxing.rs`、`contract.rs`、`perceus.rs`、`verify.rs`) に置く。translate のテストはソースから組み、ほとんどは `Pass::Translate` の直後の IR を見る。box の挿入のテストもソースから組み、ほとんどは `Pass::Boxing` の直後の IR を見る。縮約のテストと Perceus の前半のテストは、入力の IR をテキストで書き、そのパスだけをかけた出力を見る。そのパスだけをかける補助は、各ファイルの `boxing_text`、`contract_text`、`perceus_text` である。Perceus の後半のテスト、縮約の後半のテスト、translate の一部のテストは、ソースからパイプラインを通し、`core_until` で縮約や Perceus の直後の IR を見る。ソースから組むテストの表示は、`tests/common/mod.rs` の `read_back` が `parse` で読み戻し、パスに合う段の verifier にかける。`Pass::Translate` は変換の段 (`verify_translated`)、`Pass::Boxing` と `Pass::Contract` は範囲の段 (`verify_scopes`)、`Pass::Perceus` は所有の段 (`verify`) である。パスごとに見るのは、後のパスの書き換えや RC の命令を、確かめたいことと一緒に期待値に入れないためである。テキストの形の読み書きは `text.rs` で、extern の表との結び付けは `externs.rs` で確かめる
- 単体テストは、ファイルの末尾の `#[cfg(test)] mod tests` に置く。テストが300行を超え、ファイルの半分ほどを占めるようになったら、`eml_types/src/table/tests.rs` のように隣の `tests.rs` に分ける
- `crates/eml_test_support/` は、結合テストのためにパイプラインを組む関数 (`parse`、`def_map`、`lower`、`check`、`core`、`core_until`、`run`、`run_stats`、`execute`) と、診断のないことを確かめて組む関数 (`parse_clean`、`lower_clean`)、診断を文字列にする関数 (`short`、`short_text`、`full`) と fix を文字列にする関数 (`fixes`)、段階の表示に診断を足す関数 (`with_diagnostics`) を持つ。開発専用の crate で、各 crate の `tests/` からだけ使う。`src/` の `#[cfg(test)]` から使うと、テストする crate が2つ別々にリンクされて型が合わなくなる。同じ理由で `eml_types` の単体テストは `eml_cli::Session` も使えないので、`src/lib.rs` の `#[cfg(test)]` の関数 `test_program_with_files` が、読み込み、def_map、lower の段を直接つなぐ
- `parse` は構文の段 (`eml_syntax::parse`) だけを呼び、feature によらない。ほかの組む関数は、メモリ上の `(パス, 本文)` の並びを読む `MemorySource` から `eml_cli::Session` を作り、対応するメソッドを呼ぶ薄い包みである。CLI と同じ経路で段階をつなぎ、同じ診断を集めるためである。結果の診断は、読み込みの段からその段階までのすべての診断である。`def_map` と `def_map_files` は、`DefMap` と、読み込みの段と def_map の段の診断を返す。`Lowered` と `Checked` は HIR の `Program` と `Session` を持ち、ファイルを `files()` と `file()` で出す。`SourceFiles` は Clone できないためである。`core` と `core_until` は、診断にエラーがないことを確かめて `Program` を返す。`eml run` と同じく、`main` がないこともエラーである。`run` は `compile` の結果を `eml_cli::execute` に渡し、手で書いた Core IR を受け取る `execute` と、`run_stats` も同じ関数を通す。`run_stats` は、出力と、`eml_interp::RunStats` か実行時エラーを返す。`run`、`run_files`、`execute` は `RunStats` を捨てる。`RunConfig` と出力の受け口は、`eml_test_support` の1か所で組み立てる。複数のファイルのテストには `*_files` の関数 (`lower_files`、`def_map_files`、`check_files`、`core_files`、`core_until_files`、`run_files`) を使う。入口の本文と、根からの相対パス (`Report/Csv.em`) と本文の組の並びを受け取る。1つのテキストの関数は、並びが空の `*_files` と同じ経路を通る。入口の表示のパスは `ENTRY_PATH` (`test.em`) である。標準ライブラリを差し替えたプログラムは、`lower_with_std` と `check_with_std` で変換する。標準ライブラリの並び (`(パス, 本文)`) を `Session::load_with_std` に渡し、標準ライブラリの中の item の扱いを確かめるテスト (`eml_hir` の `structure.rs`、`eml_types` の `modules.rs`) が使う。並びは `Prelude.em` と本物の `Fs.em` (または同じ extern の宣言を持つもの) を含める。extern の索引が両方を引き、足りなければ panic するためである
- 段階は feature (`hir` < `types` < `core` < `run`) で選ぶ。各 feature は `eml_cli` の同じ段階までの feature を有効にし、各 crate は自分の段階までを有効にする。下流の crate がまだ組み立たなくても、上流の段階のテストを流せるようにするためである。段階の API そのものを確かめるテストは、`eml_test_support` を通さずに段階の関数を直接呼ぶ。`eml_hir` の `load.rs` と `def_map.rs` の読み込みのテスト、`item_tree.rs` の `item_tree` を直接呼ぶテスト、`eml_types` の `scaling.rs`、`eml_core_ir` の `translate.rs` の入口を選ぶテストである
- HIR と型のダンプは、Prelude だけでなく標準ライブラリのモジュールをすべて飛ばす。標準ライブラリの本文はダンプに加わらない。`println` などの extern の関数への参照は、普通の関数として表示する。ユーザーのモジュールの extern の宣言は、型、エフェクト、関数のどれも `extern` を付けて表示する (`extern data T`、`extern effect E`、`extern f : Int -> Int`)
- `eml_hir` の結合テストが、`eml_extern` の表と `std/` を照らし合わせる。表のどの行も `std/` にちょうど1回、種類の合う extern の宣言として現れること、`std/` の extern の宣言がどれも1つの行を指すこと、シグネチャの矢印の数が行の引数の数と等しいこと、`Effectful` の行だけが最後の矢印の row に extern のエフェクトを持つことを確かめる。埋め込んだ `eml_hir::STD` が `std/` のファイルと一致することも、同じ場所で確かめる
- `eml_core_ir` の結合テスト (`tests/externs.rs`) が、`eml_extern` の表の Repr を、translate の型から Repr を決める規則 (`eml_core_ir::type_repr`) で std の宣言と照らし合わせる。関数の行の `params` と `ret` が std のシグネチャの型の Repr と同じであること、型の行の `repr` が、型検査がその型に与える型の Repr と同じであること (`Unit` は空のレコード)、`by_type` でない関数の行のシグネチャに型変数がないことを確かめる。多相な extern が入る S12 で、行の Repr の比べ方を決め直す。機械の extern が配置の表を見ずに使うタグも、同じファイルで確かめる。Prelude の `Bool` の配置のコンストラクタが `[False, True]` で `FALSE` と `TRUE` に合うこと、組のタグ `TUPLE` が 0 であることである

## 層ごとの方法

| 層 | 方法 |
|---|---|
| 字句 | トークン列のスナップショット |
| パーサ | `insta` で CST をダンプしたスナップショット。壊れた入力から回復できるかのケースを多めに用意する |
| 名前解決と脱糖 | HIR の pretty printer で変換結果 (演算子の組み直しと脱糖を含む) をダンプしたスナップショット |
| 型推論 | 推論したシグネチャ (Kind と row を含む) のスナップショット |
| Core IR | Core IR の pretty printer で、`dup` / `decref` / `release` の位置を含めてダンプしたスナップショット |
| ランタイム | ヒープの単体テスト (確保と解放、世代番号による解放済みアクセスの検出、リークの数え方、長い連鎖の解放、不死の物体の数え方と数が 0 のときの誤り、文字列のその場の連結、区間を写すときの連鎖の付け替え、`release_fields` の一意と共有の側と形の誤り、参照の数の書き込みの数え方、`take_or_copy` がクロージャと継続だけを扱うこと) |
| 診断 | 表示した診断テキストのスナップショット |
| 線形性 | 線形な値 (`once` の操作の `k` など) を1回でなく使う不正なプログラム (E3001) と、正しく通るべきプログラムの対 |
| 全体 (UI テスト) | 下の「UI テスト」 |
| CLI | `eml run` / `eml check` の終了コードと引数の誤りを数件確認する |
| RC | すべての実行テストで `debug_heap` を有効にする。違反があればテストを失敗させる |

## Core IR のテキストの形

テストは、Core IR を `eml_core_ir::pretty` の表示で確かめる。`eml_core_ir::parse` はこの表示を読んで `Program` に戻し、読み直した IR を表示すると、下の往復の項の条件のもとで元の表示と同じになる。手で書く IR のテストもこの形で書く。`pretty` は extern の呼び出しの位置を出さず、`pretty_with_positions` が出す。

```
layout Prelude.Bool { False, True }
fn f(x.0: int) -> int {
  let c.1: enum = extern Prelude.<(x.0, 10)
  switch c.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
b1:
  jump b3(x.0)
b2:
  let t.2: int = extern Prelude.+(x.0, 1) @"main.em":2:20
  jump b3(t.2)
b3(t.3: int):
  return t.3
}
```

- 先頭に、データの配置を、配置の表の順に1行ずつ書く (`layout Option { None, Some(tobj) }`)。コンストラクタはタグの順に並べ、フィールドのあるコンストラクタには、宣言したフィールドの repr を括弧で書く。フィールドのないコンストラクタには括弧を書かない。空の括弧 (`None()`) は `parse` が誤りにする (`an empty field list`)。コンストラクタのない配置は `layout Void {}` と書く。組の配置は `layout (,) { (,)(tobj, tobj) }` のように、名前と同じ名前のコンストラクタを1つ持つ。`parse` は、配置にこの行の順で番号を振る
  - 同じ名前の配置が2つある (``layout `Option` is declared twice``)、`#` で始まる名前 (``a layout name cannot be `#N` ``)、形の違う組の配置 (``the tuple layout `(,)` must have one constructor `(,)` with 2 tobj fields``) は、`parse` が誤りにする。組の配置は、コンストラクタが1つで、その名前が配置と同じで、フィールドがカンマの数より1多く、すべて `tobj` でなければならない
  - 配置の行は、エフェクトの行より前に書く。エフェクトの行の後の配置の行は、関数の位置の誤り (``expected `fn` ``) になる
- 配置の行の後に、エフェクトを、エフェクトの表の順に1行ずつ書く (`effect Ask { ask/1, never stop/1 }`)。操作の名前の後には `/` と引数の数を書き、再開しない操作には `never` を付ける。操作のないエフェクトは `effect E {}` と書く。extern のエフェクト (`IO`) はエフェクトの表に入らないので書かない。`parse` は、エフェクトにこの行の順で番号を振り、操作の引数の数と、操作が再開するかどうかを戻す。
- 関数は `fn 名前(引数) -> repr { … }` と書く。内部の関数 (`CoreFn::internal`) は、`internal fn main$lambda0(..)` のように `fn` の前に `internal` を書く。repr は `obj`、`tobj`、`int`、`enum`、`unit` のどれかで、`->` の後の repr が `ret` である。関数の番号は `fn` を書いた順で、呼び出しは関数を名前で引くので、後で定義する関数も書ける。入口は `entry$main` という名前の関数で、なければ最初の関数である。文字列定数の表と、位置のパスの表 (`Program.files`) は、現れた順に作る。
- 入口のブロックにはラベルを付けない。ほかのブロックは `bN:` か `bN(引数):` の行で始め、`N` は入口を除いて書いた順に 1 から数える。順が違えば `parse` が誤りにする。`parse` は終端の後をラベルとして読み、行の字下げは読まない。`pretty` はラベルを行頭に、文と終端を2字下げて書く。
- 変数は名前と番号を `.` でつないで書く (`x.0`)。束縛する位置 (関数とブロックの引数、`let`、`unpack` と case のフィールド) では `x.0: int` のように repr を付け、使う位置 (値、`dup`、`decref`、`release` の値と残すフィールド、`save`) には付けない。`parse` は最後の `.` の後を番号とする。名前は数字で始まらず、英数字、`_`、`$`、`'` からなる。番号の先頭に 0 は書かない。表示に現れない番号は、名前が空で repr が `unit` の変数で埋める。束縛のない変数も読み、repr を `unit` にする。見えない変数の使用は verifier が報告する。同じ番号を違う repr で束縛すると、`parse` が誤りにする。
- 値は、変数、整数、`()`、タグ `#N`、関数の値 (`&` に関数の名前を続ける。`&main$lambda0`) で書く。
- 命令は配置を名前で指す。入口のモジュールの型は修飾せず (`Option`)、ほかのモジュールの型は修飾する (`Prelude.Bool`)。表にある配置も表にない配置も `#N` (配置の番号) で書ける。`pretty` は表にある配置を名前で、表にない配置を `#N` で書くので、表にある配置を `#N` で書いた IR は、読み直すと名前で表示される。表にない番号は、誤りを含む IR を verifier に渡すテストのためにある。表にない名前 (``unknown layout `Optoin` ``) と、配置を書くべき所に名前も `#N` もない形 (``expected a layout, found `(` ``) は、`parse` が誤りにする。配置のない古い形 `con #1(x.1)` は、`#1` を表にない配置と読んだ後、``expected a tag `#N`, found `(` `` で誤りになる
- 文は `let x.N: r = <右辺>`、`unpack v.N L #t(f.N: r, ..)`、`dup v.N`、`decref v.N` と書く (`unpack p.0 (,) #0(a.1: obj, b.2: obj)`)。`unpack` は、フィールドがなくても括弧を書く (`unpack p.0 Box #0()`)。verifier のテストで、フィールドのない `unpack` を書くためである。
- `release` は `release p.0 (,) #0(a.1, _)` と書く。`unpack` と同じくタグとフィールドごとの項目を書き、項目は参照を引き継ぐ変数 (repr を付けない) か `_` である。すべて `_` の `release` (`release p.0 Box #0()` を含む) と、変数でも `_` でもない項目は、`parse` が誤りにする。
- 右辺は、`call f(..)`、`apply c(..)`、`perform E.op(..)`、`perform never E.op(..)`、`resume k(v, s)`、`handle E(init, body) { 節 } return r`、`closure f(..)`、`con L #t(..)`、`const ".."`、`extern X.y(..)`、`drop a`、`box a`、`unbox a` のどれかである。
- `box` と `unbox` は、`drop` と同じく1つの値を読む (`let b.3: tobj = box n.2`、`let b.4: tobj = box 5`、`let n.2: int = unbox b.3`)。スカラーの種類は、`box` ではオペランドの repr から、`unbox` では束縛の repr から決まるので書かない。repr の誤りは `parse` でなく verifier が報告する。誤りを含む IR も読み戻して verifier に渡すためである。
- 終端は、`return a`、`tail <呼び出し>`、`jump bN(..)`、`switch a L { .. }` (タグの case を持つとき) か `switch a { .. }` のどれかである。`jump` は、引数がなくても括弧を書く (`jump b1()`)。
- `switch` は `switch o.0 Option { #0 -> b1, #1(n.2: tobj) -> b2 }` や `switch n.0 { 1 -> b1, 2 -> b2, _ -> b3 }` と書き、`String` の case は `"a" -> b1` と書く。タグの case を持つ `switch` は、scrutinee と `{` の間に配置を書く。リテラルの `switch` と、case のない `switch` には書かない。フィールドのない case は `#0` と書く。`#0()` も読むが、表示は `#0` になる。case の書き方で、読み直して表示が変わる形はこれだけである。case のない `switch` は `switch a {}` と書く。行き先のない `switch` は verifier が誤りにする。
- `apply ()(…)` と `resume ()(…)` は、呼ばれる値が `()` の `apply` と `resume` である。誤りを含む IR の表示も読み戻すためである。
- extern の呼び出しは `extern <正式な名前>(…)` と書く (`let t.2: unit = extern Prelude.println(s.1)`、`extern Prelude.+(a.0, b.1)`、`extern Std.Fs.open(p.0)`)。`parse` は正式な名前を `eml_extern` の表で引き、引数はいくつでも読む。引数の数と、引数と結果の repr が表と合うかは verifier が確かめる。誤りを含む IR も読み戻して verifier に報告させるためである。verifier は `Prelude.==` と `Prelude.!=` の `extern` も誤りにする。位置は、呼び出しの後に `@"パス":行:列` と書く。
- 文字列は `"` で囲み、エスケープ `\"`、`\'`、`\\`、`\n`、`\r`、`\t`、`\0`、`\u{…}` を読む。
- `perform` は `perform <エフェクト名>.<操作名>(…)` と書く。エフェクトの名前は `.` を含みうるが、操作の名前は含まないので、`parse` は最後の `.` でエフェクトの名前と操作の名前に分ける (`perform Report.Csv.Parse.next(t.1)`)。再開しない操作の `perform` には `never` を付ける。`parse` は `never` を書いたとおりに読み、エフェクトの表と合うかは verifier が確かめる。
- `handle` は `handle Ask(s.0, b.1) { ask: c.2 } return r.3` と書く。括弧の中は、状態の初期値と本体の関数である。節は `操作の名前: 値` をエフェクトの操作の順に並べ、節がなければ `{}` と書く。`resume` は `resume k.1(v.2, s.3)` と書き、括弧の中は値と次の状態である。
- 呼び出しの `mask` は、`let` の右辺の呼び出しと `tail` の後の呼び出しの前に、`mask [...]` で書く (`let t.2: tobj = mask [State] apply c.0(())`、`let t.3: tobj = mask [State] resume k.1(t.2, ())`、`tail mask [Report.Csv.Parse] call f(c.0)`)。エフェクトは先頭のエフェクトの行の名前で書き、入口のモジュールのエフェクトは修飾せず (`State`)、ほかのモジュールのエフェクトは修飾する (`Report.Csv.Parse`)。番号の順に並べ、飛ばす数だけ同じ名前を繰り返す (`mask [State, State]`)。エフェクトの表にない番号は、操作と同じく `#N` で書き、`pretty` も `#N` で表示する。並びの順は `parse` ではなく verifier が確かめる。`mask` のない呼び出しには何も書かない。`handle` と `perform` の前の `mask` は、`parse` が誤りにする。
- Perceus の後の呼び出しは、後ろに `save [..]` を付ける (`let t.2: int = call f(c.0) save [c.0]`)。`saved` が空なら何も書かない。
- 操作は名前のほかに `#N` (操作の番号) でも書ける。エフェクトにない番号も書けるので、誤りを含む IR を verifier に渡すテストに使う。`pretty` も、エフェクトにない番号の操作を `#N` で表示する。`handle` の中で `#N` と書いた節は、`N` がその節の 0 から数えた位置と同じでなければならない。
- `parse` は構文だけを検査する。後ろ向きの `jump`、引数の数の誤り、見えない変数の使用などの不正な IR を書け、verifier のテストに使う。空の `mask []` と `save []`、末尾のカンマ、0 で始まる番号と整数、`-0` は誤りにする。どれも、読み直して表示すると表示が変わる形だからである。`tail` の後の `save` も誤りにする。
- 往復 (pretty → parse → pretty) は、配置の名前が重ならない Program で同じテキストに戻る。
- `parse` と `pretty` は、ブロックと文の並びをループでたどり、プログラムの大きさに比例して再帰しない。

## UI テスト

- `tests/ui/run/**/*.em` は、診断のエラーなしで実行が正常に終了することを確認し、stdout と stderr をスナップショットにする
- `tests/ui/check-fail/**/*.em` は、診断のエラーが1件以上出ることを確認し、診断の表示をスナップショットにする
- `tests/ui/run-fail/**/*.em` は、診断のエラーなしでコンパイルでき、実行が実行時エラーで終わることを確認する。エラーまでの出力と実行時エラーのメッセージをスナップショットにする
- テストは `crates/eml_cli/tests/ui.rs` に置き、`insta::glob!` で `tests/ui/` 以下の `.em` を走査する。`eml_cli` の lib API をプロセス内で呼び、`debug_heap` を有効にした `RunConfig` で実行する
- テストは1つのファイル (`<最上位>/<分類>/<名前>.em`) か、1つのディレクトリ (`<最上位>/<分類>/<名前>/main.em`) である。ディレクトリのテストでは `main.em` が入口で、そのディレクトリが根になる。ディレクトリの中のほかの `.em` は `main.em` から import するモジュールで、単独のテストにしない。1つのファイルのテストはユーザーのモジュールを import しない。根が分類のディレクトリなので、import すると隣のテストのファイルをモジュールとして読んでしまう。ハーネスは、ユーザーのモジュールが入口だけであることを `Session::user_module_names` で確かめ、依存先のユーザーのモジュールを読み込んだ1つのファイルのテストを失敗にする。標準ライブラリのモジュールはいつも読み込まれるので数えず、1つのファイルのテストでも `Fs.open` や `import Fs` を使ってよい
- 成功すべきか失敗すべきかは最上位のディレクトリで決める。そのため、スナップショットの承認を誤っても、成功と失敗の入れ替わりは検出できる
- スナップショットの中のパスは `tests/ui` からの相対パスにして、実行する環境に依存しないようにする。依存先のファイルのパスも同じく `tests/ui` からの相対パスになる (`check-fail/names/import_cycle/B.em`)

### 分類

`run/`、`check-fail/`、`run-fail/` の下に、分類のサブディレクトリを切る。成功すべきか失敗すべきかは、最上位のディレクトリで決まる。`ui.rs` は、最上位のディレクトリの直下の .em、分類の直下の `main.em`、深さ3以上にあって `<分類>/<名前>/main.em` に属さない .em を拒み、テストを失敗させる。`main.em` を書き忘れたディレクトリのモジュールが、単独のテストとして黙って通るのを防ぐためである。

- `run/` と `run-fail/` は言語の機能で分け、両方で同じ名前を使う
  - `basics/`: 値、演算子、`let`、`if`、短絡評価
  - `functions/`: クロージャ、高階関数、部分適用
  - `effects/`: エフェクト、`multi`、継続
  - `data/`: `data`、コンストラクタ、`match`、パターン
  - `files/`: `Fs.File` と `Fs.open` / `Fs.read_all` / `Fs.close`、中断のときの `File` の解放、ファイルの実行時エラー
  - `modules/`: import、修飾した名前、`pub`、Prelude と標準ライブラリの修飾。ディレクトリのテストを置く
  - `runtime/`: 実装の性質を確かめるテスト。メモリの解放、スタックの深さ、合流のブロック (`join_points.em`)、末尾呼び出し、部分を共有する型 (`shared_types_*.em`。木として書き下すと列の長さの指数の大きさになる型を、長さ40の列で作る)
- 部分を共有する型の扱いが指数の時間に戻ると、`runtime/shared_types_*.em` は、終わらないかメモリを使い尽くす形で現れ、テストの失敗としては報告されない。`ui::run` は `run/` のファイルをすべて1つのテストで流すので、1つのファイルが終わらないとテスト全体が止まる
- 実行テストは入口の `.em` のあるディレクトリを `Fs.open` の基準ディレクトリにする。入力のファイルはテストの隣に置く。拡張子が `.em` でないファイルは、テストにもモジュールにも数えない
- `check-fail/` は、主なエラーの番号の範囲 ([診断](../spec/diagnostics.md) の「番号の範囲」) で分ける。機能で分けると、エフェクトの誤りのように E1xxx と E2xxx にまたがるものの置き場所が決まらないためである
  - `syntax/` (E0xxx)、`names/` (E1xxx)、`types/` (E2xxx)、`linearity/` (E3xxx)。`exhaustiveness/` (E4xxx)
  - E3001 は今は `eml_types` が出すが、出す crate ではなく番号の範囲に従って `linearity/` に置く
  - E0004 (まだ対応していない構文) は、どの段階が出しても `not-yet-supported/` に置く
- サブディレクトリの名前は、親の `check-fail` と同じくケバブケースにする
- スナップショットの名前は、最上位のディレクトリからの相対パスで固定する (`run/basics/hello.em` は `integration__ui__run@basics__hello.em.snap`。頭の `integration__ui__` は、insta が付けるテストのバイナリとモジュールの名前である)。insta の既定では、分類が1つしかないディレクトリの名前に分類が入らず、分類が増えたときに名前が変わるためである。ディレクトリのテストの名前はディレクトリのパスで、`check-fail/names/import_cycle/` は `integration__ui__check_fail@names__import_cycle.snap` になる。テストのパスが名前になるので、UI テストを移動するとスナップショットの名前が変わる。成否を変えない移動は期待値の変更で、最上位のディレクトリをまたぐ移動は成否の変更である (「テストの変更の運用」)

## 文書の引用の検査

`crates/eml_cli/tests/citations.rs` は、コメントと文書の引用を2つ確かめる。走査するのは `crates`、`docs`、`tests`、`std` である。標準ライブラリの `.em` のコメントも、`docs/…` の見出しを引ける。

- `docs/…/ファイル.md の「見出し」` の形の引用と、Markdown のリンクの直後に `の「見出し」` を続けた引用は、行き先のファイルにその見出しがある。照合では空白を無視するので、引用を折り返してもよい
- 文書の Markdown のリンクの行き先がある

見出しのない引用 (`(docs/spec/core-ir.md)` など) は検査できない。節を移すときは、そのファイルを引くコメントを grep し、移した内容に頼っているものを直す。`docs/superpowers` は作業中の設計と計画なので、検査から外す。

## CLI のテスト

`crates/eml_cli/tests/cli.rs` は `eml` バイナリを起動し、終了コードが [コンパイラの構成](architecture.md) の「CLI と lib API」の定めに合うことを確認する。

## 性能のテスト

`crates/eml_types/tests/scaling.rs` は、合成プログラムを4倍の大きさにしたときの型検査の時間の比を確かめる。形は、多相な関数の連鎖、独立した多相な関数、`data` と `match`、持ち越しの連鎖、環状の相互再帰、1つの本体で同じ名前の `let` が続く連鎖、部分を共有する型を作る `let` の列 (`let f1 = f0 ident` の列)、その列を2本作って `if` で合わせる形の8つである。HIR まで作ってから `eml_types::check` の時間だけを測り、各大きさで3回測った最小を使う。大きさは関数の数で、2000 と 8000 にする。最後の3つの形は関数の数を大きさにしない。同じ名前の `let` が続く連鎖は `let` の数を、部分を共有する型を作る2つの形は1本の列の長さを大きさにする。比が6以下なら通る。部分を共有する型を作る2つの形は、型の深さが列の長さに比例し、書き出し、単一化、occurs の検査の再帰がその深さまで進む。テストのスレッドの既定のスタック (2 MiB) では足りないので、この2つは 64 MiB のスタックのスレッドで測る。時間を測るので `#[ignore]` を付け、release ビルドで流す。型検査の構造を変えたときに流す。

`crates/eml_hir/tests/scaling.rs` は、名前の違う `data` と `effect` の宣言の数を 4000 から 16000 にしたときの、構文解析から `DefMap` までの時間の比を確かめる。比が6以下なら通る。`DefMap` の重複の判定を変えたときに流す。

`crates/eml_interp/tests/scaling.rs` は、時間ではなく、インタプリタの仕事の回数と、同時に生きていたヒープの物体の数の最大 (`RunStats`) を上限と比べる。handler の下の非末尾の再帰 (handler が1つ、内側に別のエフェクトの handler が1つ、`mask` 付きのコールバックの中) は `handler_visits` を、リテラルから始めて `acc ++ "x"` を n 回つなぐ連結は `string_bytes_copied` を、n = 2000 で n の定数倍の上限と比べる。2乗の実装では上限を超える。64 バイトのリテラルを n 回評価するテストは、`string_bytes_copied` が 64 未満であること (1回でも写せば超える) を確かめる。リストをたどるテストは `rc_increments` を確かめる。一意なリストを長さ 1000 と 2000 でたどったときに数が同じであること (セルごとの `dup` がない) と、共有されたリストを長さ n でたどったときに数が n + 2 以下であること (セルごとの `dup` は1回まで) である。ループの形ごとに、`peak_objects` を n = 1000 と n = 2000 で比べるテストもある。`Int`、`Bool`、`Unit` を返す関数の値を通るループ、呼び出しと結果の間に使われない `let` があるループ、節が末尾で再開する操作のループ、直接の自己末尾呼び出しは、2つの n で同じであることを確かめる。末尾呼び出しを失うと、フレームが反復の数に比例して増えるためである。ラムダからラムダへの末尾の `apply` は、反復ごとにクロージャが1つ残るので、n = 2000 の値が n = 1000 の値 + 1000 以下であることを確かめる。このテストはクロージャの鎖を2回使い、戻る間も鎖を共有にしておく。一意なクロージャは `apply` で手放され、そのスロットを積んだフレームが使い回すので、末尾の `apply` を失っても `peak_objects` が増えないためである。ほかに、数え方そのものを確かめるテストがある (`perform` も文字列もなければ仕事の回数はすべて 0 で `peak_objects` は `Frame::Root` の 1、`perform` は少なくとも1つのフレームを調べる、`"ab" ++ "cd"` は少なくとも4バイトを書く、型変数のフィールドに `Int` を入れて取り出すと `boxes` と `unboxes` が1以上になり、スカラーの位置だけのプログラムでは 0 になる)。各テストはソースをテストの中で作り、`eml_test_support::run_stats` で実行する。回数は機械の速さに左右されないので、`#[ignore]` を付けず、ふだんの `cargo test` で流す。

`crates/eml_interp/tests/bench.rs` は、基準のプログラム (リポジトリの `bench/`) を `debug_heap` 付きで走らせ、出力を確かめ、`RunStats` の全項目をインラインスナップショットで固定する。プログラムごとに1つのテストにして、並列に走らせる。`bench/` のファイルの一覧とテストのプログラムの一覧が一致することも確かめる。回数は決定的なので、段の前後の回数はこのスナップショットの差分で残る。回数を変える変更は、その段の spec に期待値の変更として書く。

`SourceFiles::line_col` の表のテスト (`eml_diagnostics` の `source.rs`)、verifier の大きな IR のテスト (`eml_core_ir` の `tests/verify.rs` の、長い `switch` の連鎖、借りたフィールドへの長い `switch` の連鎖、長い文の `if` の列、1つのブロックに深さの違う辺が多く合流する形)、T3 の時間のテスト (`eml_core_ir` の `tests/boxing.rs` の、N = 30,000 の関数の鎖を IR のテキストで作り、`boxing` の時間だけを測るテスト) は、数えられる仕事の回数がないので時間を測る。2乗の実装だけが超える緩い上限 (5 秒と 10 秒) を置き、`#[ignore]` を付けずに debug ビルドのふだんの `cargo test` で流す。

## feature の組み合わせの確認

ワークスペースの `cargo clippy --all-targets` は、各 crate を既定の feature でしか検査しない。`eml_cli` と `eml_test_support` の feature や、feature で切り替えるコードを変えたときは、既定でない組み合わせの lib も検査する。テストと bin は `run` を前提にするので、`--all-targets` は付けない。上流の crate のテストが下流の crate を組み立てないことは、`cargo tree -p eml_hir -e normal,dev` と `cargo tree -p eml_types -e normal,dev` に下流の段階の crate が出ないことで確かめる。

## よく使うコマンド

```sh
cargo test                                           # すべてのテスト
cargo test -p eml_syntax --test integration parser::empty_file    # 1つのテスト
cargo test -p eml_cli --test integration ui::            # UI テスト
cargo insta review                                   # スナップショットの承認
cargo clippy --all-targets && cargo fmt
cargo test --release -p eml_types --test integration scaling:: -- --ignored   # 型検査の時間の伸び (性能のテスト)
cargo test --release -p eml_hir --test integration scaling:: -- --ignored     # 名前の表を作る時間の伸び (性能のテスト)
cargo test -p eml_interp --test integration scaling::                          # インタプリタの仕事の回数 (ふだんの cargo test にも入る)
cargo clippy -p eml_cli --no-default-features --features types -- -D warnings    # 既定でない feature (なし、types、core。eml_test_support は hir も)
```
