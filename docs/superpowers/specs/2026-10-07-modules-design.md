# モジュールの設計 (M2a)

位置づけ: 作業用の設計文書。作業を終えたら削除する。

## 目的と範囲

[ロードマップ](../../future/roadmap.md) の M2 のうち、モジュールに関わる部分を実装する。複数ファイルのプログラムを `eml check` と `eml run` で検査して実行でき、UI テストでディレクトリを1件として確かめられるようにする。

M2 は2つの spec に分ける。この文書は M2a で、次のものを扱う。

- import をたどるローダ、`pub`、修飾名、モジュールとしての Prelude、`T(..)` と `E(..)`、import の循環の禁止、公開の範囲の検査、複数ファイルの UI テスト
- 同じ名前の別の型、エフェクト、コンストラクタを区別して表示すること

参照ごとの具体化の表と、`==` をそれに通すことは M2b の spec で扱う。M2b はモジュールと独立した型検査の内部の整理で、M2a の後に行う。

標準ライブラリのモジュールはまだないので、名前の解決の最後の段「標準ライブラリのモジュール」([モジュールと名前解決](../../spec/modules.md)) は M2a では入れない。

## モジュールとファイル

- 根は入口のファイルのディレクトリである。`eml run app/main.em` の根は `app/` になる
- モジュール名とパスは、大文字小文字まで完全に一致させる。`import Report.Csv` は `<根>/Report/Csv.em` を読む。今の spec の「`report/Csv.em` は `Report.Csv`」という例は、`Report/Csv.em` に直す
- 入口のモジュールの名前は、ファイル名によらず `Main` である。入口は、どの名前でも import できない。`eml run app/Server.em` の依存先が `import Server` と書くと、解決したパスが入口のファイルと同じなので E1030 にする
- `Prelude` と `Main` は予約したモジュール名である。根に `Prelude.em` や `Main.em` があっても import できない
- import は、ファイルの先頭で宣言より前に書く。宣言の後の import は parser が E0011 (「import は宣言より前に書く」) にする

```
app/
  main.em          -- Main (入口)
  Report/
    Csv.em         -- Report.Csv
    Format.em      -- Report.Format
```

## import

- `import Report.Csv` は修飾子 `Csv` を、`import Report.Csv as C` は修飾子 `C` だけを作る
- 修飾子は1つのセグメントである。パス全体を使った `Report.Csv.parse` は、修飾子 `Report.Csv` の全体を指して E1031 にする。そのモジュールを import していれば、使える修飾子 (`Csv.parse`) を help で示す
- 同じ修飾子を2つの import が作ったときは、Haskell と同じく両方のモジュールを合わせて引く。同じモジュールを2回 import しても誤りにしない
- import の並びの小文字の名前と `(op)` は値を取り込む。大文字の名前は、型かエフェクトだけを取り込む
- 並びの `(op)` は、文法どおり `:` で始まらない演算子に限る。今の parser は `((:+))` も受け付けているので、文法に合わせて E0011 にする。中置のコンストラクタは `T(..)` で取り込む
- `T(..)` は型とそのコンストラクタを、`E(..)` はエフェクトとその操作を取り込む。`E(..)` は Haskell でクラスのメソッドを `C(..)` で取り込むのと同じ形である
- 演算子の fixity は、宣言が `pub` のときに演算子と一緒に付いてくる ([宣言](../../spec/declarations.md) の「fixity」)。修飾した演算子の構文はないので、別のモジュールの演算子は import の並びで取り込んで使う

### Prelude

- Prelude は暗黙に取り込む。`Prelude` はどのモジュールでも修飾子として使え、自分の定義で隠した名前も `Prelude.map` のように引ける
- 修飾子が `Prelude` になる import は E1030 にする。`import Prelude`、最後のセグメントが `Prelude` のパス (`import Util.Prelude`)、別名の `as Prelude` である。暗黙の修飾子 `Prelude` が、ほかのモジュールと合流しないようにするためである。`import Main` も E1030 にする
- `pub` でない Prelude の item (lang item の `negate` など) は、定義がないものとして扱う。`Prelude.negate` は E1029 ではなく E1001 (「in module `Prelude`」) で、Prelude の中の位置を secondary に出さない
- Haskell の `import Prelude hiding (...)` にあたる構文はなく、Prelude の取り込み方を変える手段を設けない

## 名前の解決

修飾しない名前は次の順に引く。spec の順のうち、M2a で入れる部分である。

1. ローカルの束縛
2. 自分のモジュールのトップレベル。import した名前と Prelude の名前を、診断を出さずに隠す
3. import の並びで修飾なしにした名前
4. Prelude の `pub` の名前

- 3 で、同じ名前が別々の定義 (別の ID) を指すときだけ、その名前を使った位置で E1028 にする。同じ定義を複数の import が出しても曖昧にしない。import を書いた時点では誤りにしない
- 修飾した名前 `Q.x` は、修飾子 `Q` のモジュールだけを引き、上の順は通らない。合流した修飾子では、別々の定義が2つ以上見つかったときに E1028 にする
- 種類の決まった位置 (パターンの先頭はコンストラクタだけ、handler の節の先頭は操作だけ) では、修飾しない名前の各段でも、修飾子を通る場合でも、その種類の定義だけを見る。曖昧さも、同じ種類の定義どうしでだけ数える
- 修飾した名前は、式、型、パターン、row のエフェクト、handler の節の先頭に書ける。節の先頭は、文法を `'|' LIDENT apat* '->' body` から `'|' qvar apat* '->' body` に広げる

```haskell
-- State.em
pub effect State where
  get : Unit -> Int

-- main.em
import State

counter : Unit -> <State.State> Int
counter () = State.get () + 1

run : Int -> Int
run n =
  handle counter () with
    | State.get () k -> resume k n
    | return x -> x
```

## 公開の範囲

- `pub` を付けた item の型に、同じモジュールの `pub` でない型かエフェクトが現れたら E1032 にする。対象は、`pub` のシグネチャ、`pub data` のコンストラクタのフィールド、`pub effect` の操作のシグネチャである。row の中のエフェクトも含む
- 理由は2つある。1つは、コンストラクタを隠した抽象型を、後で `pub data` の形で意図して入れるためである ([ロードマップ](../../future/roadmap.md) の「抽象型」)。非公開の型を返す公開の関数を許すと、その前に裏口の抽象型ができてしまう。もう1つは、非公開のエフェクトを row に持つ公開の関数を、import する側が handle できないためである。その関数を呼ぶと、名前も書けないエフェクトが残る
- 検査は HIR の変換で、`pub` の item の型を解決した後に行う。診断は非公開の型かエフェクトを指し、その定義を secondary にして、`pub` を付けるよう help で伝える

## 新しい診断

| 番号 | 名前 | 内容 | 指す位置 |
|---|---|---|---|
| E1026 | `MODULE_NOT_FOUND` | import したモジュールのファイルがない、または読めない (UTF-8 でない、IO の誤り)。読めない理由をメッセージに書く | import のモジュールのパス |
| E1027 | `IMPORT_CYCLE` | import の循環 | 循環を閉じる import。循環の経路を note で示す |
| E1028 | `AMBIGUOUS_NAME` | 修飾しない名前、または合流した修飾子の名前が、別々の定義を指して曖昧 | 使った位置。候補の import を secondary にする |
| E1029 | `PRIVATE_NAME` | ユーザーのモジュールの `pub` でない名前を、修飾か import の並びで使った | 名前。定義を secondary にする |
| E1030 | `RESERVED_MODULE` | 修飾子が `Prelude` になる import、`import Main`、入口のファイルを指す import | import |
| E1031 | `UNKNOWN_QUALIFIER` | 修飾子がどの import にもない (`Foo.bar` で `Foo` を import していない)。2つ以上のセグメントの修飾子を含む | 修飾子の全体 |
| E1032 | `PRIVATE_IN_PUBLIC` | `pub` の item の型に、`pub` でない型かエフェクトが現れた | 非公開の型かエフェクトの名前。定義を secondary にする |

- モジュールにない名前を修飾か import の並びで引いたときは、新しい番号を作らず、値なら E1001、型とエフェクトなら E1002 にする。メッセージに「in module `Report.Csv`」と書く
- [診断](../../spec/diagnostics.md) の「番号を割り当てていない診断」の E1xxx の行 (修飾なしの名前の衝突) は、E1028 に割り当てて消す
- 入口のファイルが読めないことは、今と同じく CLI の使い方の誤り (終了コード 2) である。読み込みの段に入る前に `main.rs` が報告する

### 循環を禁じる理由

型検査は item と SCC の単位で動き、引数のないトップレベルの値は参照するたびに計算する。そのため、循環を許しても実装の負担はほぼない。それでも禁じるのは、モジュールの依存をいつも DAG にしておけば、インクリメンタル化、M6 の REPL の `:load`、M5 の orphan 規則をモジュールの順で考えられるためである。Haskell、OCaml、Elm、Koka、Roc も循環を禁じている。後で許すように緩めても、通っていたプログラムは通り続ける。

## 誤りからの回復

- 見つからないモジュール (E1026) と予約したモジュール (E1030) の import も、修飾子と並びの名前をスコープに登録し、壊れた import の印を付ける。壊れた import は、どの名前にも「不明」として答える
- 名前を引いたとき、壊れていないモジュールの定義がちょうど1つ見つかれば、それを使う。壊れた import を E1028 の候補には数えない。定義が1つも見つからず、合流した修飾子か競合する import のどれかが壊れていれば、診断を出さずに誤りの式にする。E1031、E1001、E1028 が連鎖しないようにするためである
- `import Missing (map)` は、手順3の名前として Prelude の `map` を隠す。その `map` を使った位置は、診断を出さずに誤りの式になる
- import の並びで、定義がない名前と `pub` でない名前は、並びの位置で1回だけ報告する。本体でその名前を使った位置は、診断を出さずに誤りにする
- 宣言の後の import (E0011) も、ほかの import と同じく読み込み、スコープに登録する。報告するのは E0011 だけである
- 曖昧な演算子 (E1028) と、壊れた import から来た演算子を含む演算子の列は、fixity で組み直さず、診断を出さずに誤りの式にする。既定の `infixl 9` で組み直すと、E1006 が連鎖しうるためである
- 循環 (E1027) を報告した後も、名前解決と型検査を続ける。item 単位の検査なので、循環があっても下流の段階は困らない

## 構成

### 読み込みの段 (`eml_hir`)

- import をたどる処理は `eml_hir` の関数にする。ファイルの読み方は `ModuleSource` という trait で引数に受け取る。`ModuleSource` は根からの相対パス (モジュールのパス) を受け取り、本文か、「ない」と「読めない (理由)」の誤りを返す
- この関数は、入口の本文を受け取って parse し、`item_tree` を作り、import を宣言の順にたどって幅優先で読む。検査するのは、E1026 (ない、読めない)、E1030 (予約した修飾子、`Main`、入口のファイルを指す import) である
- `FileId` とモジュールの番号は、Prelude が 0、入口が 1、依存先は見つけた順に 2 以降である。順が決まるので診断の並びが安定する
- `SourceFiles` に登録する表示のパスは、入口の表示のパスのディレクトリに、根からの相対パスをつないだものにする。`eml run app/main.em` なら `app/Report/Csv.em`、UI テストなら `run/modules/qualified/Report/Csv.em` になる。入口の表示のパスのディレクトリが空なら、根からの相対パスをそのまま使う
- IO は `ModuleSource` の実装だけが持つ。この関数は、同じ読み方を渡せば同じ結果を返す

### CLI (`eml_cli`)

- `eml_cli` は `FsProvider` (根のディレクトリを読む `ModuleSource`) を持つ。`FsProvider` は、パスの各段の名前がディレクトリの一覧と大文字小文字まで一致することを確かめる。macOS のように大文字小文字を区別しないファイルシステムで、`import Main` が `main.em` に当たらないようにするためである
- `main.rs` は今と同じく入口のファイルを読み、読めなければ終了コード 2 にする。`Session` は入口の表示のパスと本文と `ModuleSource` を受け取り、読み込みの段から型検査までを順に呼ぶ。`eml_cli` は段階をつなぐだけで、診断を自分では作らない
- `main` は今と同じく入口のモジュールからだけ探す

### `eml_test_support`

- メモリ上の `(パス, 本文)` の並びを読む `ModuleSource` を持ち、`hir` の feature から読み込みの段の同じ関数を呼ぶ。今の1ファイルの関数は、並びが入口だけの場合として同じ経路を通る

### `eml_hir` の名前解決

- `ItemTree` が import (モジュールのパス、別名、並び、各部の位置) を持つ。今の import の E0004 はなくす
- `def_map` は、読み込みの段が作ったモジュールの列 (名前、`ItemTree`、各 import が指すモジュールか壊れた印) を受け取る。`trees[0]` を Prelude、`trees[1]` を入口とする決め打ちと、`ModuleScope::new` の `"Prelude"` / `"Main"` の決め打ちをやめる
- `def_map` は循環 (E1027) と import の並び (E1001、E1002、E1029) を検査し、モジュールごとの import のスコープを作る。スコープは、修飾子からモジュールの列への表と、修飾なしにした名前の表である
- `lower` の `Resolver` は「名前の解決」の順で引く。`PathName::Qualified` を使う5か所 (型のパス、型の適用、row、式のパス、コンストラクタのパターン) の E0004 は、修飾した名前の解決に置き換える。handler の節の先頭も修飾を受け付ける。E1032 も `lower` で検査する
- `Program` はモジュールの列とその名前を持つ
- `pretty` は Prelude 以外のすべてのモジュールを表示する。モジュールが入口だけなら今と同じ出力にし、2つ以上のときだけ `-- Report.Csv` の見出しを付ける。入口以外のモジュールの item への参照には、`@Report.Csv.parse`、`@Prelude.not`、`Prelude.True`、`@Prelude.IO.println` のようにモジュール名を付ける。intrinsic の関数は、今と同じく修飾も `@` も付けない。解決の順を確かめるテストで、どの定義に解決したかを読み分けるためである

### `eml_syntax`

- `clause` の先頭を `qvar` にする。CST では、節の名前が `PATH` のノードになる
- 宣言の後の import と、import の並びの `(CONOP)` を E0011 にする。どちらも CST は今と同じく組む

### `eml_types`

- `dump.rs` の `module == program.entry` の絞り込みを「Prelude 以外のモジュール」に変える。`scc.rs` と `shape.rs` の同じ絞り込みは、1ファイルのテスト用のヘルパーの中にあるので変えない
- 名前の表示は次の節で扱う

## 名前の表示

### 表示名の表

- `eml_hir` の `DisplayNames` が、型、エフェクト、コンストラクタの ID から表示名を引く。`def_map` が作り、`lower` が `Program` に渡す。HIR の診断は `lower` の途中で出るので、`Program` ができる前から引けるようにするためである
- 名前は、その名前を定義するモジュールの数で数える。1つなら `Bool` と書き、2つ以上なら `Prelude.Bool`、`Main.Bool` のようにモジュール名で修飾する。同じモジュールの中の重複 (E1003) は1つと数える。型とエフェクトは同じ名前空間にあるので合わせて数え、コンストラクタは別に数える
- 継続の型の表示 `Cont` も、`Cont` という名前の定義の1つとして数える。ユーザーが `data Cont` を定義すると、そちらは `Main.Cont` と表示する
- 表示はモジュールの文脈によらないので、HIR の診断、型検査の診断、`dump`、`pretty` が同じ表を引ける
- `DisplayNames` は、ID と名前の組の並びからも作れる。`Program` を作らない単体テスト (`ty.rs` など) は、この作り方で今の期待値を保つ

### `Type` と表示

- `Type::Con` と `EffectLabel` から `name` を外し、ID だけを持たせる。型の同一性はもともと ID で決まるので、単一化と `main` の型の比較は変わらない
- `impl Display for Type` と `impl Display for EffectLabel` は、`ty.display(&names)` のような表示用のアダプタに置き換える
- 空のレコードの表示 `Unit` は、Prelude の `Unit` (lang item) の表示名を引く。ユーザーが `data Unit` を定義すると、`Prelude.Unit` と `Main.Unit` になる
- Core IR 側は、`translate/pattern.rs` の `substitute` と `translate/expr.rs` の `lang_type` から `name` を外すだけである

### 表を引く箇所

- 型検査の診断の約12か所と、`Type` を通らずに名前を直接読む4か所 (`check/report.rs` の2か所、`check/body.rs` のコンストラクタの注記、`usage.rs` の線形な状態の注記)
- HIR の診断のエフェクト名 (E1012、E1013 のメッセージ)
- 網羅性の漏れの例のコンストラクタ名 (`exhaustive.rs` の `show`)
- `dump` と `pretty`

### E1013 の help

E1013 の help に出す節の先頭は、その handler の既存の節と同じ修飾子で書く。既存の節が `| State.get () k` なら、`| State.put _ k -> ...` と示す。既存の節が修飾していなければ、今と同じく修飾しない。

### 表示が変わる場合

重なりのないプログラムでは表示が変わらない。今の既知の制限「expected `Bool`, found `Bool`」([実装の現在地](../../implementation/status.md)) は、「expected `Prelude.Bool`, found `Main.Bool`」になる。

## Core IR の名前

テキスト形式は関数とエフェクトを名前で引くので、モジュールをまたいで名前が重なってはいけない。入口以外のモジュールでは、すべての名前に `モジュール名.` を付ける。Prelude もこの規則に含める。

| 対象 | 入口 | Prelude | その他のモジュール |
|---|---|---|---|
| 関数 | `parse` | `Prelude.not` (今と同じ) | `Report.Csv.parse` |
| 操作の包み | `op$get` | `op$Prelude.open` | `op$Report.Csv.get` |
| コンストラクタの包み | `con$Row` | 該当なし (引数を持つコンストラクタがない) | `con$Report.Csv.Row` |
| エフェクトの表 | `effect Ask {…}` | `effect Prelude.IO {…}` | `effect Report.Csv.Parse {…}` |

- `builtin$` の包みは Prelude の intrinsic にしか作らず重ならないので、今の名前のままにする
- テキストの `perform` は今と同じく `perform <エフェクト名>.<操作名>(…)` と書く。操作名は `.` を含まないので、`parse` は最後の `.` でエフェクト名と操作名に分ける。今の `text.rs` は最初の `.` で分けていて、`perform Report.Csv.Parse.next()` を読めない
- 今は、ユーザーが入口で `println` という操作を定義すると、Prelude の `op$println` と名前が重なりうる。規則を1つにそろえると、この重なりもなくなる

## テスト

### 仕組み

- UI テストは今の `**/*.em` の glob を保つ。ディレクトリのテストは、ちょうど `<top>/<category>/<name>/main.em` の形に限る。`<name>/` の中の `.em` は、その `main.em` から読むモジュールで、単独のテストにしない
- 深さ3以上にあって、その形の `main.em` に属さない `.em` と、カテゴリの直下の `main.em` は、ハーネスが失敗にする。`main.em` を書き忘れたディレクトリのモジュールが、単独のテストとして黙って通るのを防ぐためである
- ディレクトリのテストのスナップショット名は、ディレクトリのパス (`names/import_cycle`) にする。`Session` に渡す入口の表示のパスは、今と同じく `tests/ui` からの相対パスにする
- 単独のファイルのテストは import を書かない
- `run` のファイルの根 (`open` が読む場所) は、今と同じくテストのディレクトリである

```
tests/ui/run/modules/
  qualified/            -- 1件
    main.em
    Report/
      Csv.em
tests/ui/check-fail/names/
  import_cycle/         -- 1件
    main.em
    A.em
    B.em
```

### 足すテスト

UI の `check-fail/` は、[テスト](../../implementation/testing.md) の規則どおり診断の番号の範囲でカテゴリを分ける。各診断の代表の表示を1件ずつ置き、端の場合は crate のテストで確かめる。

- UI の `run/modules/`: 修飾、別名、import の並び、`T(..)`、`E(..)` と修飾した節の先頭、fixity の取り込み、`Prelude.` の修飾、自分の定義で import と Prelude を隠す場合、深く入れ子になったモジュールのパス、修飾子の合流、修飾したエフェクトの row
- UI の `check-fail/names/`: E1026〜E1032 のそれぞれ、モジュールにない名前の E1001 と E1002
- UI の `check-fail/syntax/`: 宣言の後の import (E0011)
- UI の `check-fail/types/`: 同じ名前の型の表示 (E2001)
- `eml_syntax`: 修飾した節の先頭、import の位置、並びの `(CONOP)`
- `eml_hir`: `def_map.rs` (複数のモジュール、循環、import の並びの検査、`DisplayNames` の数え方)、`lower.rs` (解決の順、修飾した名前、合流と曖昧さ、種類の絞り込み、壊れた import の回復、宣言の後の import の回復、曖昧な演算子の回復、`Prelude.negate`、E1032、E1012 と E1013 の表示名と help)、読み込みの段 (たどる順と番号、E1026 のない場合と読めない場合、E1030 の各場合、入口を指す import、表示のパス)
- `eml_types`: `modules.rs` (型名の表示、`Unit` と `Cont` の表示、複数のモジュールの `dump`)、`rows.rs` (`<Log.State Int>` を import せずに書いた row が E1031 だけになること)、`exhaustive.rs` (別のモジュールの同じ名前のコンストラクタの漏れの例)
- `eml_core_ir`: `translate.rs` (名前の修飾と、入れ子のモジュールのエフェクトの `perform` と `handle`。`core_text` の往復で読み戻しも確かめる)、`text.rs` (`perform A.B.E.op()` の往復)
- `eml_cli`: `api.rs` (`FsProvider` の大文字小文字の検査。一時ディレクトリに `report/Csv.em` を置き、`import Report.Csv` がどの OS でも E1026 になること。UTF-8 でない依存先が E1026 になること。登録した表示のパス)、`cli.rs` (モジュールが見つからないときの終了コード 1、入口が読めないときの終了コード 2 が変わらないこと)

## テストの変更

テストの変更の種類は [テスト](../../implementation/testing.md) の分け方に従う。

### 種類1 (振る舞いの変更)

この spec の設計の会話と、spec の敵対的レビューを受けた見直しで合意した。

- import と修飾名の E0004 を確かめるテストを、実際の振る舞いのテストに置き換える
  - UI の `check-fail/not-yet-supported/qualified_effect.em` は消す。修飾したエフェクトの row は `run/modules/` で確かめ、import しない修飾の回復は `eml_types` の `rows.rs` で確かめる
  - `eml_hir` の `lower.rs` の `qualified_effects_are_not_supported_yet` は、`<M.E>` と `<M.State Int>` の修飾子 `M` だけを指す E1031 を2件期待するテストにする。E1002 は出ない
  - `eml_hir` の `lower.rs` の `type_and_import_are_not_supported_yet` は、`type` の E0004 だけを残す。このテストは宣言の後に import を書いているので、import は E0011 と E1026 の確認に分ける
  - そのほか `eml_hir`、`eml_types`、`eml_syntax` のコーパスなどにある import と修飾名の E0004 の期待値
- 同じ名前の型の表示
  - `eml_types` の `data.rs` の「expected `Bool`, found `Bool`」は、「expected `Prelude.Bool`, found `Main.Bool`」の形になる (向きはテストに合わせる)
  - `eml_types` の `data.rs` の `a_user_int_hides_the_prelude_int` は、「expected `Int`, found `Int`」が `Prelude.Int` と `Main.Int` に分かれる
- `run-fail/files/missing_file_through_pipe.em` の実行時エラーが `op$open` から `op$Prelude.open` になる。ソースの中のコメントの `op$open` も直す

### 種類2 (内部の表現のスナップショット)

- `eml_core_ir` の `translate.rs`、`simplify.rs`、`perceus.rs` のスナップショットで、`effect IO {` が `effect Prelude.IO {` に、`op$println` などの Prelude の操作の包みが `op$Prelude.println` などになる。ほかは変わらない
- `eml_core_ir` の `translate.rs:11-13` の assert は、`fn op$Prelude.println(` を期待する形にする。否定の assert は、Prelude のエフェクトの `perform` が出ないことを確かめる形 (`!shown.contains("perform Prelude.IO.")`) にする。名前だけを置き換えると、何も確かめない assert になるためである
- `eml_hir` の `lower.rs` と `operators.rs` の HIR のスナップショットで、Prelude の intrinsic でない関数、コンストラクタ、操作への参照に `Prelude.` が付く (`@Prelude.not`、`Prelude.True`、`@Prelude.IO.println`)
- `eml_syntax` の CST のスナップショットで、handler の節の名前が `PATH` のノードになる。`names.rs` の `a_clause_names_its_operation_with_a_name_ref` は、節の名前が `NAME_REF` でなくなるので `a_clause_names_its_operation_with_a_path` に改名する
- `eml_hir` の `tests/def_map.rs` の既存の assert は、`Resolver` の結果の型が `Option` と `Lookup` から `Resolved` に変わるので、式を書き換える (`Unusable` は `Silent`、`None` は `NotFound`、`fixity` は `Some(..)`)。確かめる意味は変えない

### 種類3 (機械的な書き換え)

期待値はバイト単位で変えない。

- `Session` と `eml_test_support` の API の変更に伴う呼び出しの書き換え
- `def_map` の引数の変更に伴う、`def_map` を直接呼ぶ箇所の書き換え (`eml_hir` の `structure.rs`、`eml_types` の `tests/modules.rs` と `test_program`)。1つの補助関数を通す形にそろえる
- `eml_syntax` の `tests/ast.rs` で、`OpClause::name` を `OpClause::path` に読み替える書き換え
- `Display` を `ty.display(&names)` に置き換えることに伴う書き換え (`eml_types` の `check.rs` の `.ty.to_string()`、`table/tests.rs`、`ty.rs` の単体テスト)

## 文書の更新

- `spec/modules.md`: 根と配置、`Main`、`Prelude.` の修飾と予約した修飾子、入口の import の禁止、`E(..)`、修飾した節の先頭、import の位置、修飾子の合流、E1028 の数え方、種類の絞り込み、公開の範囲、循環の禁止、回復
- `spec/grammar.md`: `clause` の先頭を `qvar` に、import の位置、「実装の段階」から M2 の構文を外す
- `spec/declarations.md`: `pub` の節の「import する側がまだないので効果がない」を消し、公開の範囲 (E1032) を足す
- `spec/diagnostics.md`: E1026〜E1032 を足し、「番号を割り当てていない診断」の E1xxx の行を消す。E0004 の説明から M2 を外す
- `spec/core-ir.md`: 名前の修飾と、テキストの `perform` を最後の `.` で分けること
- `spec/types.md`: 71行目の `main` の規則を「入口のモジュールに `main` があれば」にする
- `spec/examples.md`: 9行目の import を未実装とする記述を消す
- `implementation/architecture.md`: 読み込みの段と `ModuleSource`、`Session`、Prelude に定義を足して読む `load_with_prelude`。持ち上げた関数の名前 (`op$` など) の説明にモジュール名の接頭辞を足す。「採らなかった形」の `Type` から名前をなくす項目を消す。`def_map` の引数、「`Program` は Prelude と入口のモジュールを持つ」、「`SourceFiles` には Prelude と入口のファイル」を、モジュールの列に合わせて直す
- `implementation/testing.md`: ディレクトリの UI テストの形とハーネスの検査、`eml_test_support` の複数ファイルの入口と `lower_with_prelude`、コーパスの `later_stages.em` の説明 (import は実装済みになる)
- `implementation/status.md`: 今の言語の範囲。既知の制限から同じ名前の型の表示を消し、`op$open` を `op$Prelude.open` にする
- `future/roadmap.md`: M2 の節を M2b の分だけ残す
- `CLAUDE.md`: M2 を実装した構文に含める

## 範囲外

- 標準ライブラリのモジュールと、それを import なしで修飾して使う規則 (M8)
- 再エクスポート (`pub import`) と抽象型 (ロードマップの「マイルストーンの後の項目」)
- 修飾した演算子の構文 (`Csv.(</>)` など)
- 設定ファイルによる根の指定
- 根より下のモジュールを、入口を経ずに単独で検査すること。`eml check Report/Csv.em` は `Report/` を根にするので、そのファイルの import は `Report/` からの相対パスとして読む
- 網羅性の漏れの例と E1013 の help を、診断を出したモジュールで書ける形 (別名の修飾子を含む) にすること。M2a では、表示名の表の規則と既存の節の修飾子に従う
