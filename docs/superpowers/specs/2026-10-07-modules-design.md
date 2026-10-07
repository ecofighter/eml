# モジュールの設計 (M2a)

位置づけ: 作業用の設計文書。作業を終えたら削除する。

## 目的と範囲

[ロードマップ](../../future/roadmap.md) の M2 のうち、モジュールに関わる部分を実装する。複数ファイルのプログラムを `eml check` と `eml run` で検査して実行でき、UI テストでディレクトリを1件として確かめられるようにする。

M2 は2つの spec に分ける。この文書は M2a で、次のものを扱う。

- import をたどるローダ、`pub`、修飾名、モジュールとしての Prelude、`T(..)` と `E(..)`、import の循環の禁止、複数ファイルの UI テスト
- 同じ名前の別の型を区別して表示すること

参照ごとの具体化の表と、`==` をそれに通すことは M2b の spec で扱う。M2b はモジュールと独立した型検査の内部の整理で、M2a の後に行う。

標準ライブラリのモジュールはまだないので、名前の解決の最後の段「標準ライブラリのモジュール」([モジュールと名前解決](../../spec/modules.md)) は M2a では入れない。

## モジュールとファイル

- 根は入口のファイルのディレクトリである。`eml run app/main.em` の根は `app/` になる
- モジュール名とパスは、大文字小文字まで完全に一致させる。`import Report.Csv` は `<根>/Report/Csv.em` を読む。今の spec の「`report/Csv.em` は `Report.Csv`」という例は、`Report/Csv.em` に直す
- 入口のモジュールの名前は、ファイル名によらず `Main` である。入口は import できない
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

- `import Report.Csv` は修飾子 `Csv` を、`import Report.Csv as C` は修飾子 `C` だけを作る。パス全体を使った `Report.Csv.parse` は書けない
- 同じ修飾子を2つの import が作ったときは、Haskell と同じく両方のモジュールを合わせて引く。両方にある名前を修飾して使うと E1028 にする。同じモジュールを2回 import しても誤りにしない
- import の並びの小文字の名前と `(op)` は値を取り込む。大文字の名前は、型かエフェクトだけを取り込む
- `T(..)` は型とそのコンストラクタを、`E(..)` はエフェクトとその操作を取り込む。`E(..)` は Haskell でクラスのメソッドを `C(..)` で取り込むのと同じ形である
- 演算子の fixity は、宣言が `pub` のときに演算子と一緒に付いてくる ([宣言](../../spec/declarations.md) の「fixity」)。修飾した演算子の構文はないので、別のモジュールの演算子は import の並びで取り込んで使う
- Prelude は暗黙に取り込む。`Prelude` はどのモジュールでも修飾子として使え、自分の定義で隠した名前も `Prelude.map` のように引ける。`pub` でない Prelude の item (lang item の `negate` など) は、修飾しても見えない

### Prelude の明示の import

`import Prelude`、`import Main`、別名の `as Prelude` は E1030 にする。Haskell の `import Prelude hiding (...)` にあたる構文はなく、Prelude の取り込み方を変える手段を設けない。

## 名前の解決

修飾しない名前は次の順に引く。spec の順のうち、M2a で入れる部分である。

1. ローカルの束縛
2. 自分のモジュールのトップレベル。import した名前と Prelude の名前を、診断を出さずに隠す
3. import の並びで修飾なしにした名前
4. Prelude の `pub` の名前

- 3 で、2つの import が同じ名前を出したときは、その名前を使った位置で E1028 にする。import を書いた時点では誤りにしない
- 種類の決まった位置 (パターンの先頭はコンストラクタだけ、handler の節の先頭は操作だけ) では、各段でその種類の定義だけを見る。曖昧さも、同じ種類の定義どうしでだけ数える
- 修飾した名前 `Q.x` は、修飾子 `Q` のモジュールだけを引く。上の順は通らない
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

## 新しい診断

| 番号 | 名前 | 内容 | 指す位置 |
|---|---|---|---|
| E1026 | `MODULE_NOT_FOUND` | import したモジュールのファイルがない | import のモジュールのパス |
| E1027 | `IMPORT_CYCLE` | import の循環 | 循環を閉じる import。循環の経路を note で示す |
| E1028 | `AMBIGUOUS_NAME` | 修飾しない名前、または合流した修飾子の名前が、2つのモジュールから来て曖昧 | 使った位置。候補の import を secondary にする |
| E1029 | `PRIVATE_NAME` | `pub` でない名前を、修飾か import の並びで使った | 名前。定義を secondary にする |
| E1030 | `RESERVED_MODULE` | `import Prelude`、`import Main`、別名の `as Prelude` | import |
| E1031 | `UNKNOWN_QUALIFIER` | 修飾子がどの import にもない (`Foo.bar` で `Foo` を import していない) | 修飾子 |

- モジュールにない名前を修飾か import の並びで引いたときは、新しい番号を作らず、値なら E1001、型とエフェクトなら E1002 にする。メッセージに「in module `Report.Csv`」と書く
- [診断](../../spec/diagnostics.md) の「番号を割り当てていない診断」の E1xxx の行 (修飾なしの名前の衝突) は、E1028 に割り当てて消す

### 循環を禁じる理由

型検査は item と SCC の単位で動き、引数のないトップレベルの値は参照するたびに計算する。そのため、循環を許しても実装の負担はほぼない。それでも禁じるのは、モジュールの依存をいつも DAG にしておけば、インクリメンタル化、M6 の REPL の `:load`、M5 の orphan 規則をモジュールの順で考えられるためである。Haskell、OCaml、Elm、Koka、Roc も循環を禁じている。後で許すように緩めても、通っていたプログラムは通り続ける。

## 誤りからの回復

- 見つからないモジュール (E1026) と予約したモジュール (E1030) の import も、修飾子と並びの名前をスコープに登録し、壊れた import の印を付ける。そこを通る参照は、診断を出さずに誤りの式にする。E1031 と E1001 が連鎖しないようにするためである
- import の並びで、定義がない名前と `pub` でない名前は、並びの位置で1回だけ報告する。本体でその名前を使った位置は、診断を出さずに誤りにする
- 循環 (E1027) を報告した後も、名前解決と型検査を続ける。item 単位の検査なので、循環があっても下流の段階は困らない

## 構成

### ローダ (`eml_cli`)

- `Session` はファイルの読み方を `SourceProvider` として持つ。`SourceProvider` は根からの相対パスを受け取り、本文を返す trait である。`eml_cli` は `FsProvider` (根のディレクトリ) と `MemoryProvider` (`(パス, 本文)` の並び) を公開する
- `FsProvider` は、パスの各段の名前がディレクトリの一覧と大文字小文字まで一致することを確かめる。macOS のように大文字小文字を区別しないファイルシステムで、`import Main` が `main.em` に当たらないようにするためである
- `Session::new(provider)` で作り、`check(entry)` と `compile(entry)` は最初に読み込みの段を走らせる。入口を parse して `item_tree` を作り、import を宣言の順にたどって幅優先で読む。ファイルが見つからなければ E1026 を、予約したモジュールなら E1030 を出す
- `FileId` とモジュールの番号は、Prelude が 0、入口が 1、依存先は見つけた順に 2 以降である。順が決まるので診断の並びが安定する。入口が今と同じモジュール 1 なので、Core IR と `dump` の既存のスナップショットもほぼ変わらない
- IO は読み込みの段だけが持つ。HIR から後の段階は、今と同じく純粋な関数である

### `eml_hir`

- `ItemTree` が import (モジュールのパス、別名、並び、各部の位置) を持つ。今の import の E0004 はなくす
- `def_map` は、モジュールの列 (名前と `ItemTree`) と、各 import が指すモジュールの番号を受け取る。`trees[0]` を Prelude、`trees[1]` を入口とする決め打ちと、`ModuleScope::new` の `"Prelude"` / `"Main"` の決め打ちをやめる
- `def_map` は循環 (E1027) と import の並び (E1001、E1002、E1029) を検査し、モジュールごとの import のスコープを作る。スコープは、修飾子からモジュールの列への表と、修飾なしにした名前の表である
- `lower` の `Resolver` は「名前の解決」の順で引く。`PathName::Qualified` を使う5か所 (型のパス、型の適用、row、式のパス、コンストラクタのパターン) の E0004 は、修飾した名前の解決に置き換える。handler の節の先頭も修飾を受け付ける
- `Program` はモジュールの列とその名前を持つ。`Program::main` は今と同じく入口だけを探す

### `eml_syntax`

- `clause` の先頭を `qvar` にする。CST では、節の名前が `PATH` のノードになる
- 宣言の後の import を E0011 にする。`ItemKind` の import は、宣言が現れた後に読むと誤りになる

### `eml_types`

- `module == program.entry` の絞り込み (`scc.rs`、`shape.rs`、`dump.rs`) を「Prelude 以外のモジュール」に変える
- 型名の表示は次の節で扱う

### `eml_core_ir`

- 名前の修飾は「Core IR の名前」の節で扱う

## 型名の表示

- `Type::Con` と `EffectLabel` から `name` を外し、ID だけを持たせる。型の同一性はもともと ID で決まるので、単一化と `main` の型の比較は変わらない
- `Program` から `TypeNames` を作る。`TypeNames` は、型とエフェクトの ID から表示名への表である。型とエフェクトは同じ名前空間にあるので、両方を合わせて数える。プログラム全体でその名前の定義が1つなら `Bool` と書き、2つ以上なら `Prelude.Bool`、`Main.Bool` のようにモジュール名で修飾する。表示はモジュールの文脈によらないので、診断と `dump` が同じ表を引ける
- `impl Display for Type` と `impl Display for EffectLabel` は、`ty.display(&names)` のような表示用のアダプタに置き換える
- 表を引くのは、診断の約12か所と、`Type` を通らずに名前を直接読む4か所 (`check/report.rs` の2か所、`check/body.rs` のコンストラクタの注記、`usage.rs` の線形な状態の注記) と、`dump` である
- `dump` は Prelude 以外のすべてのモジュールを表示する。モジュールが入口だけなら今と同じ出力にし、2つ以上のときだけ `-- Report.Csv` のような見出しを付ける
- Core IR 側は、`translate/pattern.rs` の `substitute` と `translate/expr.rs` の `lang_type` から `name` を外すだけである

重なりのないプログラムでは表示が変わらない。今の既知の制限「expected `Bool`, found `Bool`」([実装の現在地](../../implementation/status.md)) は、「expected `Prelude.Bool`, found `Main.Bool`」になる。

## Core IR の名前

テキスト形式は関数とエフェクトを名前で引くので、モジュールをまたいで名前が重なってはいけない。入口以外のモジュールでは、すべての名前に `モジュール名.` を付ける。Prelude もこの規則に含める。

| 対象 | 入口 | Prelude | その他のモジュール |
|---|---|---|---|
| 関数 | `parse` | `Prelude.not` (今と同じ) | `Report.Csv.parse` |
| 操作の包み | `op$get` | `op$Prelude.open` | `op$Report.Csv.get` |
| コンストラクタの包み | `con$Row` | 該当なし (引数を持つコンストラクタがない) | `con$Report.Csv.Row` |
| エフェクトの表 | `effect Ask {…}` | `effect Prelude.IO {…}` | `effect Report.Csv.Parse {…}` |

今は、ユーザーが入口で `println` という操作を定義すると、Prelude の `op$println` と名前が重なりうる。規則を1つにそろえると、この重なりもなくなる。

## テスト

### 仕組み

- `eml_test_support` に、複数ファイルを受け取る入口を足す (`check_files(&[("main.em", …), ("Report/Csv.em", …)])` など)。今の1ファイルの関数は、1ファイルの並びとして同じ経路を通す
- UI テストは今の `**/*.em` の glob を保つ。カテゴリより下で `main.em` を持つディレクトリの中のファイルは、その `main.em` だけを1件として扱い、ほかの `.em` は単独のテストにしない。スナップショット名はディレクトリのパスにする。`Session` に登録するパスは、今と同じく `tests/ui` からの相対パスにする
- `run` のファイルの根は、今と同じくテストのディレクトリである

```
tests/ui/run/modules/
  qualified/            -- 1件
    main.em
    Report/
      Csv.em
tests/ui/check-fail/modules/
  cycle/                -- 1件
    main.em
    A.em
    B.em
```

### 足すテスト

- UI の `run/modules/`: 修飾、別名、import の並び、`T(..)`、`E(..)` と修飾した節の先頭、fixity の取り込み、`Prelude.` の修飾、自分の定義で import と Prelude を隠す場合、深く入れ子になったモジュールのパス、修飾子の合流
- UI の `check-fail/modules/`: E1026〜E1031 のそれぞれ、モジュールにない名前の E1001 と E1002、宣言の後の import (E0011)、同じ名前の型の表示、壊れた import を通る参照が連鎖しないこと
- `eml_syntax`: 修飾した節の先頭、import の位置
- `eml_hir`: `def_map.rs` (複数のモジュール、循環、import の並びの検査)、`lower.rs` (解決の順、修飾した名前、曖昧さ、回復)
- `eml_types`: `modules.rs` (型名の表示、複数のモジュールの `dump`)
- `eml_core_ir`: `translate.rs` (名前の修飾)
- `eml_cli`: `cli.rs` (モジュールが見つからないときの終了コード 1)

## テストの変更

テストの変更の種類は [テスト](../../implementation/testing.md) の分け方に従う。

### 種類1 (振る舞いの変更)

この spec の設計の会話で合意した。

- import と修飾名の E0004 を確かめるテストを、実際の振る舞いのテストに置き換える。UI の `check-fail/not-yet-supported/qualified_effect.em` は消し、修飾したエフェクトの row を `run/modules/` のテストで確かめる。`eml_hir` の `lower.rs` などにある import と修飾名の E0004 のテストも、解決のテストに置き換える
- 「expected `Bool`, found `Bool`」を確かめるテスト (`eml_types` の `data.rs`) が、修飾した名前になる
- `run-fail/files/missing_file_through_pipe.em` の実行時エラーが、`op$open` から `op$Prelude.open` になる
- 宣言の後に import を書いた parser のテストがあれば、E0011 になる

### 種類2 (内部の表現のスナップショット)

- Core IR のスナップショットで、Prelude の操作とコンストラクタの包みと、`effect IO` に `Prelude.` が付く
- `eml_syntax` の CST のスナップショットで、handler の節の名前が `PATH` のノードになる

### 種類3 (機械的な書き換え)

- `Session::new` と `eml_test_support` の API の変更に伴う呼び出しの書き換え。期待値はバイト単位で変えない

## 文書の更新

- `spec/modules.md`: 根と配置、`Main`、`Prelude.` の修飾と明示の import の禁止、`E(..)`、修飾した節の先頭、import の位置、修飾子の合流、循環の禁止、回復
- `spec/grammar.md`: `clause` の先頭を `qvar` に、import の位置、「実装の段階」から M2 の構文を外す
- `spec/diagnostics.md`: E1026〜E1031 を足し、「番号を割り当てていない診断」の E1xxx の行を消す。E0004 の説明から M2 を外す
- `spec/declarations.md`: `pub` の節の「import する側がまだないので効果がない」を消す
- `spec/core-ir.md`: 名前の修飾
- `implementation/architecture.md`: ローダと `Session`、`SourceProvider`
- `implementation/testing.md`: ディレクトリの UI テスト、`eml_test_support` の複数ファイルの入口
- `implementation/status.md`: 今の言語の範囲、既知の制限から同じ名前の型の表示を消す
- `future/roadmap.md`: M2 の節を M2b の分だけ残す
- `CLAUDE.md`: M2 を実装した構文に含める

## 範囲外

- 標準ライブラリのモジュールと、それを import なしで修飾して使う規則 (M8)
- 再エクスポート (`pub import`) と抽象型 (ロードマップの「マイルストーンの後の項目」)
- 修飾した演算子の構文 (`Csv.(</>)` など)
- 設定ファイルによる根の指定
- 根より下のモジュールを、入口を経ずに単独で検査すること。`eml check Report/Csv.em` は `Report/` を根にするので、そのファイルの import は `Report/` からの相対パスとして読む
