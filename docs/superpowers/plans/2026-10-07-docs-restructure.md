# 文書の整理 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `docs/` を、経緯を含まずに単独で読め、1つの事実を1つの文書にだけ書いた文書の集まりに組み直し、S2 の出発点にする。

**Architecture:** ファイルの配置は今のままにし、`docs/implementation/test-changes.md` だけを削除する。3段で進める。段1で `status.md` を作り直してその中身を行き先へ移し、段2で spec と future と architecture の重複と古い記述を直し、段3で入口 (`README.md`、`overview.md`、CLAUDE.md、メモリ) を合わせる。

**Tech Stack:** Markdown (日本語)、grep と zsh のスクリプトによる検査、yomiyasu のリンター。

**Spec:** `docs/superpowers/specs/2026-10-07-docs-restructure-design.md`

## Global Constraints

- コードとテストは変えない (`crates/`、`tests/` に差分を出さない)
- 文書は日本語の常体 (である調) で書く。英単語の前後の半角空白など、`docs/` の既存の書き方に合わせる
- 日本語を書く前に `yomiyasu:yomiyasu` スキルを読み込み、その規則に従う
- 経緯 (どの段階、どの回で入れたか) を書かない。「マイルストーン1 では」「マイルストーン1 の時点から」は「今は」「最初から」などの今の状態の言い方にする。語「マイルストーン1」を残すのは、`overview.md` の用語の行と `status.md` の冒頭の1文だけ
- 構文の段階の名前 S1、S2、S3 は残してよい
- 行番号はすべて整理を始める前のもの。編集の前に、引用した文で場所を確かめる

## Review Focus

- 移した項目の取りこぼし: `status.md` から消した注意点が、行き先のどこにもない。Task 1 Step 5 の検査で全項目を確かめる
- 切れたリンク: 消した節 (「次の作業の注意点」「リファクタリング」など) や `test-changes.md` を指すリンクが残る。各 Task の終わりにリンクの検査を流す
- 重複を消したときの規則の欠落: 寄せる側に、消した側にしかなかった規則が書かれていない (例: `linearity.md` の「操作の引数の Kind」の理由)。Task 2 Step 2 で、消す前に寄せる側に同じ内容があるかを確かめる
- 経緯の言葉の残り: 「段階4a」「R7」「マイルストーン1 では」などが残る。Task 3 Step 6 の grep で確かめる
- 新しい `status.md` の言い切りが実装と食い違う: 「実装したもの」の列挙に、実装していない機能が入る。Task 1 Step 1 で、旧 `status.md` の「含めるもの」と段階の表の内容から外れていないかを確かめる

## 共通の検査コマンド

リンクの検査 (相対リンクがすべて実在するか)。出力が `done` だけなら通る。

```sh
cd /Users/arakaki/Projects/eml && for f in $(git ls-files 'docs/*.md' CLAUDE.md) $(git ls-files --others --exclude-standard 'docs/*.md'); do [ -e "$f" ] || continue; grep -oE '\]\([^)#]+' "$f" | sed 's/](//' | grep -v '^http' | while read l; do [ -e "$(dirname $f)/$l" ] || echo "$f -> $l"; done; done; echo done
```

経緯の言葉の検査。

```sh
cd /Users/arakaki/Projects/eml && grep -rnE 'マイルストーン1|段階[0-9]|\bR[0-9]|暫定構文|test-changes|次の作業の注意点|vertical-slice steps' docs CLAUDE.md | grep -v 'docs/superpowers'
```

---

### Task 1: `status.md` を作り直し、中身を行き先へ移す

**Files:**
- Modify: `docs/implementation/status.md` (全体を書き直す)
- Modify: `docs/future/roadmap.md`
- Modify: `docs/spec/exhaustiveness.md`
- Modify: `docs/spec/diagnostics.md`
- Modify: `docs/implementation/testing.md:5,20-21,24`
- Modify: `docs/README.md:29`
- Delete: `docs/implementation/test-changes.md`

**Interfaces:**
- Produces: `status.md` の節の名前「今の言語の範囲」「構文の段階」「既知の制限」「S2 の材料」。Task 2 と Task 3 はこの名前でリンクする

- [ ] **Step 1: `docs/implementation/status.md` を次の内容で書き直す**

````markdown
# 実装の現在地

位置づけ: 手引き。

マイルストーン1 (言語の全体を一通り通す vertical slice) と構文の段階 S1 は完了した。この文書は、今の言語の範囲、構文の段階、既知の制限、次の作業 (S2) の材料をまとめる。実装が進んだら更新する。

## 今の言語の範囲

### 実装したもの

- 基本型: `Int`、`Bool`、`String`、`Unit`。`Bool` は Prelude の `data Bool = | False | True` で、キーワード `true` / `false` はない。単項の `!` はなく、関数 `not` を使う
- タプル (数字ラベルの閉じたレコード。[直積型とレコード](../spec/records.md))、型引数を持つ代数的データ型 (`data`)、`match` と入れ子のパターン、`Int` と `String` のリテラルのパターン
- トップレベルの関数 (シグネチャが必須)、複数の等式による定義、ローカルの `let` と `let ... in`、`if` (then 節が `Unit` なら `else` を省略できる)、再帰、ラムダとクロージャ、部分適用、型の明示 `(e : T)`
- シグネチャの型変数と row 変数による多相
- fixity の宣言とユーザー定義の演算子、演算子のセクション、`use`、最後の引数のラムダ
- `drop` キーワード
- エフェクトの宣言 (`never` / `once` / `multi`、型引数)、`handle` (deep)、`resume`、`drop k`、パラメータ付き handler (`handle ... from ... with`)
- `IO` エフェクト (Prelude の `pub effect IO`) の `println`、`open`、`read_all`、`close` と、線形型 `File`
- 線形性、持ち越し規則、網羅性の検査
- 入力は1つのファイルで、Prelude を別のモジュールとして読み込む

### まだないもの

- 浮動小数と文字 (字句として予約し、使うと E0004。[ロードマップ](../future/roadmap.md))
- S2 と S3 の構文 (下の「構文の段階」)
- 型クラスなどのアドホック多相、可変参照 `Ref`、表面の構文での Kind の記述 (関数型の線形性 `m` を含む)、`IO` をユーザーが handle すること
- or パターン、ガード、as パターン
- Perceus の reuse analysis と借用の最適化
- LSP、フォーマッタ、REPL、LLVM バックエンド

将来の拡張は [ロードマップ](../future/roadmap.md) にまとめてある。

## 構文の段階

本番の構文は3段階で実装する。

| 段階 | 内容 | 状態 |
|---|---|---|
| S1 | lexer (`--` と `{- -}`、shebang、`'` を含む識別子、数値の形)、レイアウト段、上の「実装したもの」の文法 (宣言、式、パターン、型と row、エスケープだけの通常の文字列) | 完了 |
| S2 | 名前付きのレコードと `type`、モジュールと import / `pub`、`Prelude`、リストのリテラル、補間、複数行の文字列、raw 文字列 | 未着手 |
| S3 | コマンドリテラル (標準ライブラリの `Proc` と一緒に) | 未着手 |

S2 と S3 の構文は、字句と文法の置き場所を用意してあり、使うと E0004 (未対応) を出して回復する。どの層が E0004 を出すかは [文法](../spec/grammar.md) の「実装の段階」にある。

## 既知の制限

### 診断の表示と言い方

- `IO` の操作を値として渡したとき (`|>`、`<|`、`>>` を通したときを含む) の実行時エラーは、操作を包む関数の Core IR の名前 (`op$open` など) で報告される。[ロードマップ](../future/roadmap.md) の「実行時エラーの位置」で、利用者向けの関数名を付けるときに直す
- ariadne は、同じファイルの区画の見出しに、どの区画でも primary の位置を出す (`ariadne` 0.6 の `write.rs`)。そのため、secondary が primary より前の行にあって区画が分かれると (E3002 の「first used here」、E1003 の「first defined here」など)、2つ目の区画の見出しも primary の行と列になる。設定では変えられないので、直すときは表示を ariadne 以外で組むか、ariadne を書き換える
- scoped labels で同じラベルが重なると、E2002 のメッセージが実際とずれる。例えば `run_io : (Unit -> <e> Unit) -> <IO | e> Unit` を、`<IO>` の `main` から `IO` を起こすコールバックで呼ぶと、呼び出しの row は `<IO, IO>` になる。このとき「`main` のシグネチャは `IO` を許さない」と報告するが、`main` は `IO` を1つ許しており、許さないのは重なった2つ目の `IO` である
- HIR が呼び出しを1つにまとめるので、`(1 + 1) 2` は「`+` は2引数なのに3引数が渡された」と報告される。呼び出しをまとめるのは、`(f a) b` と `x |> f a` を引数の揃った1つの呼び出しとして扱うためである。まとめた呼び出しで引数が余ったときは、元の呼び出しの区切りで報告する方がよい
- 同じ名前の別の型を区別して表示しない。ユーザーが Prelude の `Bool` と同じ名前の `data Bool` を宣言すると、`if` の条件などで「expected `Bool`, found `Bool`」になる。直し方は下の「S2 の材料」にある

### 深さと性能

- 非常に長い平らな演算子の列 (約1万項) は、同じ深さの HIR の木になる。型検査と Core IR への変換がその木を再帰するので、スタックがあふれる。演算子を E0013 の深さに数えるか、平らに保つ
- 決定木の再帰 (`translate/pattern.rs` の `decide`) の深さは、1つの `match` のパターンの大きさで決まる。コンストラクタやタプルの入れ子の深さは E0013 で抑えられるが、決定木の再帰の深さそのものは抑えていない。リテラルの列は1つの `Switch` になるので、リテラルが1000個の `match` は debug ビルドでも動く。一方、複数の葉から届く `match` の枝の join point は互いの範囲に入れ子になるので、そうした枝の多い `match` (`match (a, b) with | (0, 0) -> 0 | (_, 1) -> 1 … | (_, 999) -> 999 | _ -> 7`) は debug ビルドでスタックがあふれる
- 型検査の段2は、1つの SCC の中で、関数ごとに残す成分から残さない領域をたどる。DAG の形によっては、残さない成分を関数ごとに何度もたどる。たとえば環状の相互再帰で、各関数の矢印の Kind 変数がどれも定数の境界を持つと、その成分は何もない成分にならず、関数の数の2乗の時間がかかる。また、持ち越しの制約を関数ごとに SCC 全体から見るので、関数の数と持ち越しの制約の数の積がかかる。どちらも、大きな SCC が多くの持ち越しの制約を持つ場合だけに起きる

### 型と row の推論

- `let` で束縛したラムダの row は、最初の呼び出しで決まる。ローカルの `let` は単相で、関数型の局所変数の型は開かない ([型と Kind](../spec/types.md) の「推論」) ためである。例えば `<IO>` の本体で `let g = fn x -> x + 1` を呼んでから `g` を `(Int -> Int)` の引数に渡すと、E2001 か E2002 になる。どちらになるかは、呼び出しと受け渡しの順で変わる。`let` で束縛したラムダの値の row 変数だけを多相化する案があり、これには spec の決定が要る
- 持ち越し規則は、開いた row の呼び出しを今の row と同じとみなす。`include_row` が、末尾が推論用の変数の row を今の row とまるごと単一化するためである。例えば `<Choice, IO>` の本体で `let log = fn () -> println "x"` を呼ぶと、`log` の row が `<Choice, IO>` になり、`log ()` をまたいで持っている `File` が E3006 になる。エフェクト多相な関数に純粋な関数を渡す `keep f (fn () -> ())` も同じである。`handle` と `resume` の外側の row も handle の位置の今の row なので、今の row に `multi` があれば、本体が `multi` の操作を起こさなくても、handle をまたぐ `File` が拒否される。どれも健全な側に外れるだけである。`let` で束縛したラムダの row を閉じて使う位置で開くか、row の包摂を入れるか、handle の本体が起こすエフェクトを本体の中の呼び出しの row から集めるときに見直す

## S2 の材料

S2 の spec を書くときの出発点である。S2 を終えたら、この節を次の段階の材料に入れ替える。

### 決めること

- row の仕組みを sort 付きの1つにし、エフェクトの row とレコードの row で共有する
- 名前付きのレコードの実行時の表し方 ([直積型とレコード](../spec/records.md)、[Core IR とインタプリタ](../spec/core-ir.md))
- 文字列のトークンを lexer のモードで分ける。lexer のモードのスタックは、補間のトークンと一緒に設計する。未対応のリテラルの E0004 を1か所で出すかどうかも、そのときに決める
- レイアウト規則3とレコードの `with` の衝突を解く
- 借用のオペランドと `Field` を決める
- 参照ごとの具体化を記録する表を作り、`==` の比べ方を一般化する
- 同じ名前の別の型を区別して表示する方法 (上の「既知の制限」)。Prelude に `Option` や `List` が入ると重なりが増える。案は、`Type` から名前をなくし、表示する側が `Program` から名前を引いて修飾することである。`Display`、診断の文言、`dump` の経路は変わるが、後の段階は `DeclType::ty` を読むだけなので、使う側は変わらない。REPL で `data T` を定義し直したときも同じことが起きる
- import の循環を許すかどうかと、モジュールの根

### 作るもの

- import をたどるローダ、複数ファイルの fixture、ディレクトリを1件とする UI テスト
- `::` の fixity
- 補間、コマンドリテラル、レコード、リストの CST
- spec で決めてあり実装を待つもの: import の並びでコンストラクタを `T(..)` でだけ取り込む規則 ([モジュールと名前解決](../spec/modules.md))、タプルの射影 `t.0`、名前付きのレコード、射影と更新の線形性の規則 ([直積型とレコード](../spec/records.md))
````

確かめ方: 旧 `status.md` の「含めるもの」(旧 :21-28) と段階の表 (旧 :66-75) にない機能を「実装したもの」に書いていないこと。`git show HEAD:docs/implementation/status.md | sed -n '17,75p'` と見比べる。

- [ ] **Step 2: `docs/future/roadmap.md` に将来の課題を移す**

2-1. 冒頭の段落 (旧 :5) を次に置き換える。

```markdown
今の実装の後に扱う論点と将来の拡張を、分野ごとにまとめる。方針まで決まっている項目と、論点を挙げただけの項目がある。詳しい設計がある項目は、その文書へのリンクを付けた。今の言語の範囲は [実装の現在地](../implementation/status.md) の「今の言語の範囲」にある。
```

2-2. 「型システム」の「アドホック多相」の項目 (旧 :12-14) の子の箇条の最後に、次の1行を足す。

```markdown
  - `==` と `!=`: 今は `Int`、`String`、`Bool` だけで比べられる。`data` の型とタプルの等値と、多相な関数の中の `==` は、`Eq` の制約で扱う。`String` の順序の比較 (`<` など) もそのときに決める
```

2-3. 「型システム」の最後 (旧 :19 の後) に、次の3項目を足す。

```markdown
- 浮動小数と文字: `Float` と `Char` の型、リテラル、Prelude の演算。字句は予約してあり、今は使うと E0004 になる ([字句](../spec/lexical.md))。算術と比較の演算子を型ごとに分けるか、アドホック多相で共有するかを一緒に決める
- フィールドの関数型の線形性: `data` のフィールドに書いた関数型の線形性は、表面の構文で `m` を書けないので `Unr` に固定している ([型と Kind](../spec/types.md))。線形なクロージャを `data` に入れられるようにするかを見直す
- 操作の引数の Kind 変数: 操作の引数の型に現れる Kind 変数は `Unr` に固定している ([エフェクトと handler](../spec/effects.md) の「handler の意味」)。線形な値を多相な操作の引数で渡せるようにするかを見直す
```

2-4. 「言語機能と構文」の最後 (旧 :31 の後) に、次の1項目を足す。

```markdown
- 仮置きの式: どの型にもなる仮置きの式 (typed hole)。入れるときに、網羅されていない `match` と等式の fix (E4001 の枝の追加、E4002 の等式の追加の提案) を一緒に入れる ([診断](../spec/diagnostics.md) の「網羅性の診断」)
```

2-5. 「処理系」の「Perceus の最適化」(旧 :39) を次に置き換える。

```markdown
- Perceus の最適化: reuse analysis (FBIP)、借用パラメータ。あわせて次の2点を見直す
  - Core IR の変数は Kind を持たず、ボックス化した変数はすべて Perceus の対象になる。`File` も RC で数え、`read_all` と `close` は一意性を求めないので正しく動く。`Lin` の変数を Perceus の対象から外すかを、借用と reuse と一緒に決める
  - 変数の枝が scrutinee そのものを受ける `match` (`| ys -> f ys`) では、Perceus が `Switch` の前に scrutinee を `dup` するので、一意な箱でも `take_or_copy` の写す経路を通る。結果は正しいが、reuse analysis で写さずに済むようにする
```

2-6. 「再帰する join point」の子の箇条 (旧 :43-44) を直す。
- 旧 :43 の「入れる時期: マイルストーン1 では入れない。」を「入れる時期: 今は入れない。」にする
- 旧 :44 の「join point の引数を複数にする変更は、段階4a で `match` のために入れた」を「join point はすでに複数の引数を持てる」にする

2-7. 「simplify の書き直し」(旧 :46) の「([実装の現在地](../implementation/status.md) の「次の作業の注意点」)」を消し、項目の最後に次の文を足す。

```markdown
不動点まで繰り返すことにしたときは、B3 が長い続きの連鎖を `Switch` の枝の中へ移しうる。Perceus と verifier は `Switch` の枝を再帰でたどるので、E0013 はその深さの上限にならなくなる。
```

- [ ] **Step 3: `docs/spec/exhaustiveness.md` を直す**

3-1. 「検査パス」の最後の箇条 (旧 :80 の「等式の `match` は、…」) の後に、次の箇条を足す。

```markdown
- 漏れているパターンの例は、現れていないコンストラクタを見つけた段階で、現れているコンストラクタの中の漏れを探さない (Maranget の作り方どおり)。そのため、note の例が漏れのすべてを表さないことがある (`| Some (Some _) -> …` の漏れは `None` だけが出る)。
```

3-2. 「範囲外」(旧 :96) を次に置き換える。

```markdown
or パターン、ガード、as パターンはまだない ([実装の現在地](../implementation/status.md) の「今の言語の範囲」)。
```

- [ ] **Step 4: `docs/spec/diagnostics.md` に「他の言語の書き方へのヒント」を足す**

「番号を割り当てていない診断」の節の最後の段落 (「シグネチャに関する E1xxx の診断には、…」) の後に、次の節を足す。

```markdown
## 他の言語の書き方へのヒント

他の言語の書き方を前提にした help や Warning は出さない。例えば Haskell 形式の `\x ->` のラムダに対するヒントはない。診断が煩雑になるだけのためである。
```

- [ ] **Step 5: `testing.md` と `README.md` から `test-changes.md` を外し、削除する**

5-1. `docs/implementation/testing.md` 旧 :5 の「テストの変更の記録は [test-changes.md](test-changes.md) にある。」を消す。

5-2. 「テストの変更の運用」の表の「合意と記録」の列を次にする。

| 種類 | 合意と記録 |
|---|---|
| 1. 振る舞いの変更 | `事前に合意を取る。変えた理由は、その作業の spec とコミットメッセージに書く` |
| 2. 内部表現の変更 | `作業の spec に、変わるテストと理由を列挙する。spec の承認を合意とみなす` |
| 3. 機械的な追随 | 変えない |

5-3. 旧 :24 の箇条から「過去には、テストを守るために別の enum の種類を足したり、spec に例外を足したりしたことがある ([status.md](status.md) の「リファクタリング」)」を消す。

5-4. `docs/README.md` 旧 :29 の `test-changes.md` の行を消す。旧 :30 の `status.md` の行の「内容」を「今の言語の範囲、構文の段階、既知の制限、S2 の材料」にする。

5-5. 削除する。

```sh
git rm -q docs/implementation/test-changes.md
```

- [ ] **Step 6: 移した項目がすべて行き先にあることを確かめる**

次を実行する。各行の出力が1件以上あれば通る。`MISSING` が出たら、その項目を行き先に足す。

```sh
cd /Users/arakaki/Projects/eml && while IFS='|' read file key; do grep -qF -- "$key" "$file" && echo "ok      $file: $key" || echo "MISSING $file: $key"; done <<'EOF'
docs/implementation/status.md|op$open
docs/future/roadmap.md|仮置きの式
docs/implementation/status.md|ariadne
docs/future/roadmap.md|フィールドの関数型の線形性
docs/implementation/status.md|約1万項
docs/implementation/status.md|`decide`
docs/future/roadmap.md|E0013 はその深さの上限にならなくなる
docs/implementation/status.md|sort 付きの1つ
docs/implementation/status.md|lexer のモード
docs/implementation/status.md|レイアウト規則3
docs/implementation/status.md|借用のオペランド
docs/implementation/status.md|参照ごとの具体化
docs/future/roadmap.md|操作の引数の Kind 変数
docs/implementation/status.md|expected `Bool`, found `Bool`
docs/spec/exhaustiveness.md|Maranget の作り方どおり
docs/future/roadmap.md|take_or_copy
docs/future/roadmap.md|Core IR の変数は Kind を持たず
docs/implementation/status.md|最初の呼び出しで決まる
docs/implementation/status.md|scoped labels
docs/implementation/status.md|段2は
docs/implementation/status.md|(1 + 1) 2
docs/implementation/status.md|t.0
docs/future/roadmap.md|`String` の順序
docs/implementation/status.md|include_row
docs/implementation/status.md|T(..)
docs/spec/diagnostics.md|`\x ->`
docs/implementation/status.md|import をたどるローダ
docs/implementation/status.md|`::` の fixity
docs/future/roadmap.md|浮動小数と文字
EOF
```

- [ ] **Step 7: リンクの検査を流す**

「共通の検査コマンド」のリンクの検査を流す。この時点では、`status.md` の消えた節の名前を引くリンクは文字列の検査に出ないので、出力が `done` だけなら通る。

- [ ] **Step 8: コミットする**

```sh
git add -A docs && git commit -q -F - <<'EOF'
Rewrite status.md as the current state and move its notes to their owners

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01RTHAQHrziF2SaFqSesitsj
EOF
```

---

### Task 2: spec、architecture、future の重複と古い記述を直す

**Files:**
- Modify: `docs/spec/linearity.md:15-16,19,35,39`
- Modify: `docs/spec/effects.md:24-25,63,73`
- Modify: `docs/spec/expressions.md` (「handler」の `try` の例、旧 :180)
- Modify: `docs/spec/declarations.md:127,149`
- Modify: `docs/spec/lexical.md:54,73`
- Modify: `docs/spec/grammar.md:116`
- Modify: `docs/spec/modules.md:5`
- Modify: `docs/spec/core-ir.md:74,99`
- Modify: `docs/spec/types.md:18,20`
- Modify: `docs/spec/runtime.md:5,13,18,47-56`
- Modify: `docs/spec/records.md:80,82`
- Modify: `docs/spec/diagnostics.md:112`
- Modify: `docs/implementation/architecture.md:28,84,97-99,122,129-139,183`
- Modify: `docs/implementation/testing.md` (補助関数、テストの地図、UI テストの分類)
- Modify: `docs/future/multicore.md:5,34,138,236-258,247,268-270,278`
- Modify: `docs/future/evidence-passing.md:5,60,85,96,98`

**Interfaces:**
- Consumes: Task 1 の `status.md` の節の名前
- Produces: `runtime.md` の節の名前「マルチコアに備えた予防的な決定」(旧「マイルストーン1 に入れた予防的な決定」)。`multicore.md` と `README.md` がこの名前で引く

- [ ] **Step 1: `linearity.md` を直す**

- 旧 :15 を次に置き換える。

```markdown
- handle の本体と `return` の節が変数を捕まえることも、handle 式の位置での1回の使用に数える。操作の節が捕まえる変数に付く制約と、`return` の節が捕まえた値の持ち越しは、[エフェクトと handler](effects.md) の「handler の意味」で定める。
```

- 旧 :16 を次に置き換える。

```markdown
- 操作の引数の型に現れる Kind 変数は `Unr` に固定する。規則と理由は [エフェクトと handler](effects.md) の「handler の意味」で定める。
```

- 旧 :19 の「マイルストーン1 では表現しない」を「今は表現しない」にする。
- 旧 :35 (「パラメータ付き handler の状態」の段落) を次に置き換える。

```markdown
パラメータ付き handler の状態は、節の中では普通の引数なので、線形性の検査がそのまま当てはまる。状態が `Lin` なら、各節で `resume` に渡すか、`close` / `drop` しなければならない。`drop k` する節でも、状態の後始末を忘れればエラーになる。handle の本体を実行している間の状態の持ち越しは [エフェクトと handler](effects.md) の「パラメータ付き handler」で、構文は [式](expressions.md) の「パラメータ付き handler」で定める。
```

- 旧 :39 の1文目と2文目を次に置き換える (3文目の「複数の等式で定義した関数は、…」は残す)。

```markdown
- 型推論の後に、型付き HIR の上で別のパスとして行う。使用回数のパスと持ち越しのパスは、関数ごとに本体をたどって Kind の制約を集める。Kind の制約は呼び出し関係を通じて伝わるので、制約は呼び出しグラフの SCC ごとに解き、違反はそこで見つかる ([型と Kind](types.md) の「推論」)。
```

- [ ] **Step 2: `effects.md` を直す**

消す前に、寄せる先の `declarations.md` の「`effect`」に、E1015 と、型引数と同じ名前の型変数の扱いと、E1008 があることを確かめる (旧 `declarations.md` :82-86)。

- 旧 :24 を次に置き換える。

```markdown
- エフェクトは型引数を持てる (`effect State s`)。row に付ける型引数の個数 (E1015) と、操作のシグネチャの型変数の扱いは [宣言](declarations.md) の「`effect`」で定める。
```

- 旧 :25 を次に置き換える。

```markdown
- `never` の操作の結果の型は自由な型変数なので、呼び出した側ではどんな型として使ってもよい。結果の型に書けるもの (E1008) は [宣言](declarations.md) の「`effect`」で定める。
```

- 旧 :63 の「マイルストーン1 で組み込みにする操作と型は次のとおり。」を「組み込みの操作と型は次のとおり。」にする。
- 旧 :73 の最後に「`file_root` の既定 (空のパス) はカレントディレクトリを指す。」を足す (Step 8 で `architecture.md` から消す文の受け先)。

- [ ] **Step 3: `expressions.md`、`declarations.md`、`lexical.md`、`grammar.md`、`modules.md` を直す**

- `expressions.md` の「handler」の節の `try` のコードブロック (旧 :120-127 の ```` ```haskell ```` から ```` ``` ```` まで) を、次の1文に置き換える。

```markdown
例は [エフェクトと handler](effects.md) の「handler の意味」にある。
```

- `expressions.md` 旧 :180 の「並行処理はマイルストーン1 の範囲外である。」を「並行処理はまだ実装していない ([マルチコア対応の設計](../future/multicore.md))。」にする。
- `declarations.md` 旧 :127 の「マイルストーン1 では `==` と `!=` を除いて」を「今は `==` と `!=` を除いて」にする。
- `declarations.md` 旧 :149 の1文目「`pub` は R7 で受け付ける。」を消す。
- `lexical.md` 旧 :54 の表の見出し「マイルストーン1 での扱い」を「今の扱い」にする。
- `lexical.md` 旧 :73 の「マイルストーン1 以降の当面の間、」を「当面の間、」にする。
- `grammar.md` 旧 :116 を次に置き換える。

```markdown
文法はすべての段階の構文を含む。まだ実装していない機能 (レコード、モジュールなど) も構文の置き場所を先に決めてあるのは、後から入れても文法を作り直さずに済むようにするためである。S1 で実装した文法は次のとおり。
```

- `grammar.md` 旧 :122 の「段階の全体は [実装の現在地](../implementation/status.md) にまとめてある。」を「段階の全体は [実装の現在地](../implementation/status.md) の「構文の段階」にある。」にする。
- `modules.md` 旧 :5 の「マイルストーン1 は単一ファイルである。」を「今の実装は単一ファイルである。」にする。

- [ ] **Step 4: `core-ir.md`、`types.md`、`records.md`、`diagnostics.md` を直す**

- `core-ir.md` 旧 :74 の「マイルストーン1 で入れるパスは、」を「今あるパスは、」にする。
- `core-ir.md` 旧 :99 の最後の文「`drop k` で捨てた区間、…`debug_heap` を有効にした実行テストで確かめている。」を消す。
- `types.md` 旧 :18 の「マイルストーン1 から、内部の束は `Never ≤ Once ≤ Multi` の3要素で実装する。」を「内部の束は、最初から `Never ≤ Once ≤ Multi` の3要素で実装する。」にする。
- `types.md` 旧 :20 の「(操作の引数の型と同じ扱い。見直しは [実装の現在地](../implementation/status.md) に記録する)」を「(操作の引数の型と同じ扱い。見直しは [ロードマップ](../future/roadmap.md) にある)」にする。
- `records.md` 旧 :80 の「マイルストーン1 で使う直積型は」を「今実装している直積型は」にする。旧 :82 の「マイルストーン1 では、タプルは」を「今は、タプルは」にする。
- `diagnostics.md` 旧 :112 の1文目を次に置き換える (2文目以降は残す)。

```markdown
E0004 (`NOT_YET_SUPPORTED`) は、まだ実装していない構文に使う。S2 と S3 の構文と、字句として予約した浮動小数と文字のリテラルである。
```

- [ ] **Step 5: `runtime.md` を直す**

- 旧 :5 の「マイルストーン1 の時点から入れておく決定 (予防的な決定)」を「最初から入れておく決定 (予防的な決定)」にする。
- 旧 :13 の「マイルストーン1 のヒープは」を「今のヒープは」にする。
- 旧 :18 の「マイルストーン1 では常に正である。」を「今は常に正である。」にする。
- 旧 :47 の見出しを「## マルチコアに備えた予防的な決定」にする。
- 旧 :49 を「マルチコア対応はまだ実装しない。ただし、後で作り直さずに済むよう、次の決定を守る。」にする。
- 旧 :55 の「(crate は 8 つ)」を消す。旧 :56 の「マイルストーン1 では常に正」を「今は常に正」にする。

- [ ] **Step 6: `multicore.md` を直す**

- 旧 :5 の2文目と3文目を次に置き換える。

```markdown
マルチコア対応はまだ実装しない。後で作り直さずに済むよう今から守る予防的な決定は、[ランタイム](../spec/runtime.md) の「マルチコアに備えた予防的な決定」にまとめ、規範としては同じ文書などの spec に反映してある。
```

- 旧 :34 の表の「マイルストーン1 のヒープ」を「今のヒープ」にする。
- 旧 :138 の「マイルストーン1 では、内部の Kind の束を」を「今の実装は、内部の Kind の束を」にする。
- 旧 :247 の「- マイルストーン1:」を「- 今の実装:」にする。
- 「### 規約」と「### 実行の API」の2つの小節 (旧 :250-258) を消し、その位置に次の段落を置く。

```markdown
### 規約と実行の API

インタプリタの値とフレームに `Rc` と `RefCell` を使わないこと、`run` の出力先を `Send + Sync` にすること、`RunConfig` に `threads` と `schedule_seed` を足すことは、[ランタイム](../spec/runtime.md) の「マルチコアに備えた予防的な決定」で定める。各段階は「グローバルな可変状態を持たない純粋な関数」という規律 ([コンパイラの構成](../implementation/architecture.md)) を守る。
```

- 旧 :268-270 の節の見出しを「## マルチコアに備えた予防的な決定」にし、本文を「作り直しが要らないよう、今から入れておく決定がある。一覧と規範としての記述は、[ランタイム](../spec/runtime.md) の「マルチコアに備えた予防的な決定」にまとめてある。」にする。
- 旧 :278 の「**局所的な可変変数の糖衣構文**: …」の箇条を「**局所的な可変変数の糖衣構文**: [ロードマップ](roadmap.md) の「言語機能と構文」にある」にする。

- [ ] **Step 7: `evidence-passing.md` を直す**

- 旧 :5 の「マイルストーン1 では実装しない。マイルストーン1 に入れた予防的な決定は、」を「まだ実装しない。今の実装に入れた予防的な決定は、」にする。
- 旧 :60 の「マイルストーン1 で状態を脱糖しないことにしたのは」を「状態を脱糖しないことにしたのは」にする。
- 旧 :85 の「段階5で入れる、区間の解放の中で破棄処理を呼ぶ仕組みと同じ考え方である」を「インタプリタが継続の区間を解放するときに、捕まっていた値を解放する (`File` はその解放で閉じる) のと同じ考え方である」にする。
- 旧 :96 の「今は Core IR に最適化のパスがなく、row を失う変換もないので、evidence passing を入れるときに HIR から Core IR への変換で足せばよい」を「今の Core IR は row を持たないので、evidence passing を入れるときに HIR から Core IR への変換で足し、`simplify` などのパスがそれを保つようにする」にする。
- 旧 :98 の「(段階6で実装する)」を「(実装済み)」にする。

- [ ] **Step 8: `architecture.md` を直す**

- 旧 :28 の「([status.md](status.md) の「次の作業の注意点」)」を「([status.md](status.md) の「S2 の材料」)」にする。
- 旧 :84 の「現在の各段階の入口は次のとおり (実装状況は [status.md](status.md))。」を「現在の各段階の入口は次のとおり。」にする。
- 旧 :97-99 の3つの箇条 (パーサの `ERROR` ノード、回復の同期点、`Error` 型) を、次の1つの箇条に置き換える (:96 と :100 は残す)。

```markdown
- パーサは壊れた入力でも `ERROR` ノードを作って回復し、パニックしない。名前解決と型推論は、誤りの場所に `Error` 型を入れて診断の連鎖を抑える。規則は [レイアウト規則](../spec/layout.md) と [診断](../spec/diagnostics.md) の「連鎖する診断の抑止」にある
```

- 旧 :122 の最後の文「S1 の時点では、補間を含む文字列などをそれぞれ1つのトークンにし、トークン列への分割はこれらを実装する S2、S3 で行う」を「今は、補間を含む文字列などをそれぞれ1つのトークンにしている。トークン列への分割は、これらを実装する S2 と S3 で行う」にする。
- 「## `eml_hir` で行う脱糖と検査」の節 (旧 :129-139) の本文を、次の段落に置き換える (見出しは残す)。

```markdown
HIR への変換では、名前解決に加えて、名前の重複と未定義の名前の検査、シグネチャと等式の対応の検査、spec が HIR に割り当てた脱糖と検査を行う。式の脱糖と検査の一覧は [式](../spec/expressions.md) の「HIR で行う脱糖と検査」に、宣言とレコードに関するものは [宣言](../spec/declarations.md) と [直積型とレコード](../spec/records.md) にある。実装上の判断は下の「`eml_hir` の内部」に書く。
```

- 旧 :183 の最後の文「`RunConfig::file_root` は `open` の基準ディレクトリで、既定の空のパスはカレントディレクトリを指す」を消す (受け先は Step 2 の `effects.md`)。

- [ ] **Step 9: `testing.md` のテストの地図と分類を実際に合わせる**

- 「crate の中の置き方」の `crates/eml_test_support/` の箇条で、「診断を文字列にする関数 (`short`、`short_text`、`full`)」を「診断を文字列にする関数 (`short`、`short_text`、`full`) と fix を文字列にする関数 (`fixes`)」にする。
- 「今あるテストの地図」の `eml_hir` の行の結合テストに、`item_tree.rs` の前に「`eval.rs` (呼び出しの評価の手順と、引数をまとめて渡す範囲)、」を足す。
- 「分類」の `run/` と `run-fail/` の箇条の子に、`data/` の行の後へ次の行を足す。

```markdown
  - `files/`: `File` と `open` / `read_all` / `close`、中断のときの `File` の解放、ファイルの実行時エラー
```

- [ ] **Step 10: 経緯の言葉とリンクを検査する**

「共通の検査コマンド」の経緯の言葉の検査を流す。この時点で残ってよいのは、`docs/overview.md`、`docs/README.md`、CLAUDE.md、`docs/implementation/status.md` の冒頭の1文 (「マイルストーン1 (言語の全体を…」) だけである。ほかの行が出たら、Global Constraints の言い方の規則で直す。続けてリンクの検査を流し、出力が `done` だけなら通る。

- [ ] **Step 11: 変えた文書に yomiyasu のリンターをかける**

```sh
cd /Users/arakaki/Projects/eml && for f in $(git diff --name-only HEAD -- docs); do echo "== $f"; python3 /Users/arakaki/.claude/plugins/cache/yomiyasu/yomiyasu/1.0.4/skills/yomiyasu/scripts/yomiyasu_lint.py "$f" | grep -E 'ERROR|bold_not_rendered' ; done
```

`bold_not_rendered` が出たら案のとおりに直す。英単語の前後の半角空白と箇条書きの比率の指摘は、`docs/` の既存の書き方なので直さない。

- [ ] **Step 12: コミットする**

```sh
git add -A docs && git commit -q -F - <<'EOF'
Give each rule one owner across the docs and drop stale milestone wording

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01RTHAQHrziF2SaFqSesitsj
EOF
```

---

### Task 3: 入口を合わせる (`README.md`、`overview.md`、CLAUDE.md、メモリ)

**Files:**
- Modify: `docs/README.md:54-55`
- Modify: `docs/overview.md:68-75,93-95`
- Modify: `CLAUDE.md` (Architecture 節の最後の項目、Testing 節)
- Delete: `/Users/arakaki/.claude/projects/-Users-arakaki-Projects-eml/memory/m1-skeleton-deferred-followups.md`
- Delete: `/Users/arakaki/.claude/projects/-Users-arakaki-Projects-eml/memory/syntax-s1-deferred-followups.md`
- Modify: `/Users/arakaki/.claude/projects/-Users-arakaki-Projects-eml/memory/MEMORY.md`

**Interfaces:**
- Consumes: Task 1 の `status.md` の節の名前、Task 2 の `runtime.md` の節の名前

- [ ] **Step 1: `README.md` を直す**

- 旧 :54 を「- `implementation/status.md` は実装の現在地を書く。実装が進んだら更新する。」にする。
- 旧 :55 の「マイルストーン1 に入れた予防的な決定は、」を「今の実装に入れた予防的な決定は、」にする。

- [ ] **Step 2: `overview.md` を直す**

- 「### コンパイラと実行系」の表 (旧 :68-75) を、次の段落に置き換える。

```markdown
処理系はバッチ型のパイプラインで、各段階を純粋な関数にし、後でクエリ化 (salsa など) できるようにしてある。実行系は、型付き Core IR (ANF 形式で、RC とエフェクトを明示する) を CEK 風のインタプリタで実行する。メモリは Perceus 方式の参照カウントで管理し、`Lin` の値には RC の操作を付けない。マルチコア対応はまだ実装せず、予防的な決定だけを今の実装に入れてある。詳細は [コンパイラの構成](implementation/architecture.md)、[Core IR とインタプリタ](spec/core-ir.md)、[ランタイム](spec/runtime.md)、[マルチコア対応の設計](future/multicore.md) にある。
```

- 用語の表の旧 :93 を「| マイルストーン1 (M1) | 言語の全体を一通り通した最初の vertical slice。完了している。今の範囲は [実装の現在地](implementation/status.md) にある |」にする。
- 用語の表の旧 :94 (「暫定構文」の行) を消す。
- 用語の表の旧 :95 の「S1 は M1 の機能の文法」を「S1 は M1 の機能の文法 (完了)」にする。

- [ ] **Step 3: CLAUDE.md を直す**

- `## Architecture` の最後の箇条を次に置き換える。

```markdown
- `eml_syntax` implements syntax stage S1 (`docs/implementation/status.md`). S2/S3 constructs (records, modules, interpolation, command literals, ...) and the reserved float and char literals are lexed and parsed far enough to report E0004; where each one is reported is listed in `docs/spec/grammar.md`.
```

- `## Testing` の「Kinds 1 and 2 are recorded in `docs/implementation/test-changes.md`.」を「For kinds 1 and 2, write the reason in the work's spec and the commit message.」にする。
- `## Testing` の `eml_test_support` の箇条を次に置き換える。

```markdown
- `eml_test_support` (dev-only) builds the pipeline for integration tests (`parse` / `lower` / `def_map` / `check` / `core` / `core_until` / `run` / `execute`, and `parse_clean` / `lower_clean` that assert a clean source; `lower_clean` needs `hir`) and formats diagnostics (`short` / `short_text` / `full`, fixes with `fixes`, joined to a stage dump with `with_diagnostics`). Core IR tests are written as IR text read by `eml_core_ir::parse`. Use it only from `tests/`, never from `#[cfg(test)]` in `src/`: the crate under test would be linked twice. Stages are features (`hir` < `types` < `core` < `run`); each crate enables only up to its own stage, so its tests still build while downstream crates are broken mid-refactor.
```

- [ ] **Step 4: 古いメモリを消す**

2つのメモリは「未決の項目を `status.md` の「未決の論点」で追う」と書いているが、その節はもうなく、項目も解決済みである。2つのファイルを削除し、`MEMORY.md` から次の2行を消す。

```markdown
- [M1 skeleton deferred follow-ups](m1-skeleton-deferred-followups.md) — skeleton open findings now tracked in docs/implementation/status.md
- [Syntax S1 deferred follow-ups](syntax-s1-deferred-followups.md) — S1 open decisions/minors now tracked in docs/implementation/status.md
```

- [ ] **Step 5: リンクを検査する**

「共通の検査コマンド」のリンクの検査を流す。出力が `done` だけなら通る。

- [ ] **Step 6: 経緯の言葉を検査する**

「共通の検査コマンド」の経緯の言葉の検査を流す。出てよいのは次の2行だけである。

- `docs/overview.md` の用語の表の「マイルストーン1 (M1)」の行
- `docs/implementation/status.md` の冒頭の「マイルストーン1 (言語の全体を一通り通す vertical slice) と構文の段階 S1 は完了した。」

- [ ] **Step 7: テストを流す**

```sh
cargo test 2>&1 | grep -E '^test result' | awk '{p+=$4; f+=$6} END {print "passed",p,"failed",f}'
```

Expected: `failed 0`。コードは変えていないので、通る件数は整理の前と同じ (905)。

- [ ] **Step 8: コミットする**

```sh
git add -A docs CLAUDE.md && git commit -q -F - <<'EOF'
Point the docs entry points and CLAUDE.md at the restructured docs

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01RTHAQHrziF2SaFqSesitsj
EOF
```

---

### Task 4: 作業用の文書を削除する

- [ ] **Step 1: spec と計画を削除してコミットする**

```sh
git rm -q docs/superpowers/specs/2026-10-07-docs-restructure-design.md docs/superpowers/plans/2026-10-07-docs-restructure.md && git commit -q -F - <<'EOF'
Delete the docs restructure design and plan

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01RTHAQHrziF2SaFqSesitsj
EOF
```
