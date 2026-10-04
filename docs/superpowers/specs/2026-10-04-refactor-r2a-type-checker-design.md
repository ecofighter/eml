# リファクタリング R2a: 型検査器の内部の設計

位置づけ: 作業用の設計文書。R2a を終えたら、残す価値のある内容を `docs/implementation/architecture.md` と `docs/implementation/status.md` に移し、この文書は削除する。

## 目的と範囲

[status.md](../../implementation/status.md) の「R1〜R3 で直す項目」の R2 は、性質の違う2つの塊からなる。1つは型検査器の内部の整理で、もう1つは HIR から Core IR までにまたがるデータモデルの作り直しである。R2 を R2a (型検査器の内部) と R2b (データモデル) に分け、R2a を先に行う。R2b は `table.rs` と `check.rs` を大きく書き換えるので、先にファイルを分けて名前を直しておくと、R2b の差分が読みやすくなる。

R2a は `eml_types` の中に閉じる。リファクタリング全体の方針 (UI テストの出力は原則として変えない、段階3〜5の器の形は作り替えるが機能は実装しない) と、テストの変更の運用 ([testing.md](../../implementation/testing.md)) に従う。

R2a で振る舞いが変わるのは2点で、どちらもユーザーと合意済みである。1つは、row の末尾に `Error` を置くことで、`status.md` にある既知の誤り2件が直る。もう1つは、E2002 の副ラベルを spec の定めのとおり矢印に向ける。

## 1. `table.rs` の分割、改名、row の末尾の `Error`

### 分割

`crates/eml_types/src/table.rs` (1061行) を、`crates/eml_types/src/table/` のモジュールに分ける。

| ファイル | 中身 |
|---|---|
| `table/mod.rs` | 型と変数の ID (`Ty`、`TyVar`、`RigidVar`、`RowVar`)、`TyShape`、`ArrowLin`、`Row`、`Tail`、`UnifyError`、`Table` の構造体と格納 (`alloc`、新しい変数の生成、`resolve`、`shape`) |
| `table/unify.rs` | 型の単一化 (`unify`、`bind_var`、`occurs`、`unify_arrow_lin`) |
| `table/row.rs` | row の単一化と束縛 (`unify_row`、`bind_row`、`resolve_row`)、`include_row`、`open_spine` |
| `table/kinds.rs` | Kind の制約 (`kind_bounds`、`kind_at_most`、`closure_kinds`、`kind_vars`、制約の残りと写し、`solve_kinds`) |
| `table/copy.rs` | `Subst` と、スキームの具体化に使う `copy_type` |
| `table/export.rs` | `export`、`display`、`kind_names` |
| `table/tests.rs` | 今の `table.rs` の単体テスト |

`impl Table` は各ファイルに分けて書く。分割は関数の移動であり、処理は変えない (下の改名と `Tail` を除く)。

### 改名

| 今 | 新しい名前 | 理由 |
|---|---|---|
| `TyKind` | `TyShape` | Kind は線形性と多重度を指す言葉で、型の形ではない |
| `Table::kind(ty)` | `Table::shape(ty)` | 同上 |
| `Mult` | `ArrowLin` | 中身は関数の矢印の線形性 `m` で、多重度 (`Multiplicity`) ではない |
| `Table::fresh_mult()` | `Table::fresh_arrow_lin()` | 線形性の束から変数を取り、`ArrowLin::Var` で返す |
| `Table::fresh_lin_kind()` | `Table::fresh_lin_var()` | 線形性の束の Kind 変数である |
| `Table::fresh_mult_kind()` | `Table::fresh_mult_var()` | 多重度の束の Kind 変数である |
| `Table::unify_mult` | `Table::unify_arrow_lin` | `ArrowLin` の単一化である |

### row の末尾の `Error`

今は、シグネチャの row に未定義のエフェクトか解決できない row 変数があると、HIR の `RowRef::Error` を新しい推論用の row 変数にし、その変数を `Rigids::error_rows` と `Scheme::error_rows` で引き回している。具体化のたびに、その変数を新しい変数に置き換える。この仕組みには2つの誤りがある ([status.md](../../implementation/status.md) の「次の作業の注意点」)。

- 未定義のエフェクトを持つシグネチャの関数は、本体が起こしたエフェクトをその row 変数に取り込み、呼び出し側に伝えてしまう。例えば `g : Unit -> <Console> Unit` を `g () = println "x"` と定義して純粋な関数から呼ぶと、E1002 に加えて E2002 が出る。
- 具体化の写しどうしが、多重度の Kind 変数 `σ` を共有している。

これを、型の `Error` と同じ考え方に置き換える。

- 内部の row を `Row { labels: Vec<Effect>, tail: Tail }` にし、`Tail` を `Closed`、`Var(RowVar)`、`Error` の3つにする。今の `tail: Option<RowVar>` の `None` は `Closed`、`Some(v)` は `Var(v)` になる。
- 末尾が `Error` の row は、どのエフェクトも受け入れ、束縛されない。
  - `unify_row` で一方の末尾が `Error` なら、相手の row のラベルの過不足を問わずに成功する。相手の末尾が推論用の row 変数なら、その変数を、こちらにしかないラベルと末尾 `Error` からなる row に束縛する。型の `unify(Var, Error)` が変数を `Error` に束縛するのと同じである。
  - `include_row` で呼び出し先か今の row の末尾が `Error` なら、成功する。
  - `copy_type` は末尾 `Error` をそのまま写す。変数がないので、具体化の写しどうしで共有するものがない。
- HIR の `RowRef::Error` は、末尾が `Error` の row に変換する。`Rigids::error_rows`、`Scheme::error_rows`、`lower_signature` の `error_rows` の引数、`instantiate` の中でエラーの row を新しい変数に替える処理を消す。
- 外に出す型 (`eml_types::RowTail`) に `Error` を足し、`{error}` と表示する。型の `Error` の表示 `{error}` と同じ形である。今は `_` (解けなかった変数と同じ表示) なので、区別できるようになる。

この変更で、上の誤りの1つ目は直り、`g` の例では E1002 だけが出る。2つ目は、変数がなくなるので構造上起きなくなる。

### 表示のために Kind の束を解かない

今の `export` は、`solve_kinds` の前に呼ばれると、矢印の線形性を求めるたびに `Lattice::value` で束全体を解く。型の不一致の診断を作るときと `check_main` で呼ばれているが、型の表示は線形性を出さない。

- `Table::display(ty) -> Type` を作る。推論用の Kind 変数を解かず、矢印の線形性はすべて `Linearity::Unr` にする。診断の文言 (`mismatch`、ラムダの矢印の数の食い違い) と `check_main` は `display` を使う。`main` の型は一番外側の矢印が `Unr` に決まっていて、内側に矢印がないので、`display` で比べても結果は変わらない。
- `Table::export(ty) -> Type` は、`solve_kinds` の後にだけ呼ぶ。`solve_kinds` の前に呼ばれたら `debug_assert` で止める。`TypedModule` の組み立ては、今も `solve_kinds` の後に行っている。

## 2. `check.rs` の整理と E2002 の副ラベル

### 分割

`crates/eml_types/src/check.rs` (830行) を、`crates/eml_types/src/check/` のモジュールに分ける。

| ファイル | 中身 |
|---|---|
| `check/mod.rs` | `check_module` (SCC の順の検査、多相化、`TypedModule` の組み立て)、`check_main`、`has_error`、`kind_constraints` |
| `check/body.rs` | `BodyCheck` と `BodyTyping`。式、文、呼び出し、ラムダ、パターンの検査 |
| `check/report.rs` | `Origin`、`AmbientSource` と、診断を作る関数。型の不一致、E2002、引数と矢印の数の食い違い (シグネチャ、ラムダ、呼び出し)、`callee_subject`、`count` |

### 重複をなくす

- `check_expr` と `infer_expr` にある `If` と `Block` の処理を1つにする。`Expectation` を次の2つの enum にする。
  - `Has(Ty, Origin)`: 期待する型とその由来がある
  - `None`: 期待する型がない
  
  `fn if_expr(&mut self, id, condition, then_branch, else_branch, expectation: Expectation) -> Ty` と `fn block(&mut self, stmts, tail, range, expectation: Expectation) -> Ty` を作り、`check_expr` は `Has` で、`infer_expr` は `None` で呼ぶ。`Has` のときの振る舞いは今の `check_expr` の腕と、`None` のときの振る舞いは今の `infer_expr` の腕と同じにする。
- 矢印をたどる3つのループ (`check_function`、`check_lambda`、`call`) の共通部分を `fn next_arrow(&mut self, ty: Ty) -> Arrow` にまとめる。`Arrow` は次の3つの enum である。
  - `Fn { param, row, ret }`
  - `Error`
  - `NotFunction`
  
  型が推論用の変数なら、新しい関数型と単一化してから `Fn` を返す (今の `call` と `check_lambda` の処理)。シグネチャの型は推論用の変数を含まないので、`check_function` で使っても振る舞いは変わらない。`NotFunction` のときの診断の文言は3つの場所で違うので、呼び出し側が `report.rs` の関数を呼んで作る。
- ラムダの検査で今の row とその由来を保存して戻す処理を、`fn with_ambient<T>(&mut self, row: Row, source: AmbientSource, check: impl FnOnce(&mut Self) -> T) -> T` にまとめる。段階3の handler でも同じ処理が要る。期待する型が壊れているときのラムダの row は、新しい row 変数ではなく、末尾が `Error` の row にする (1の「row の末尾の `Error`」と同じ考え方)。

### 名前

呼び出しの row を今の row に含める `BodyCheck::perform` を、`include_call_row` に改名する。エフェクトの `perform` や Core IR の `Rhs::Perform` と紛れないようにするためである。

### E2002 の副ラベル

[診断](../../spec/diagnostics.md) は、E2002 が「シグネチャの矢印を指す」と定めている。本体のエフェクトが入るのは、引数の数だけ矢印をたどった最後の矢印の row なので、指すのはその矢印の部分の型である。今の実装はシグネチャの型全体を指している。

- `AmbientSource::Signature` の副ラベルの範囲を、シグネチャの型の注釈 (`TypeRef`) を引数の数より1つ少ない回数だけ `ret` 側にたどった `TypeRefKind::Fn` の範囲にする。例えば `f : Int -> Int -> <IO> Unit` を `f a b = ...` と定義すると、`Int -> <IO> Unit` の部分を指す。
- たどる途中で `TypeRefKind::Fn` でない注釈に当たった場合と、引数がない場合は、今と同じくシグネチャの型全体を指す。
- ラムダの戻り値の場合 (`AmbientSource::Lambda(Origin::Return)`) の副ラベルは、今のままシグネチャの型全体を指す。spec が定めているのはシグネチャの矢印の場合だけである。

既存のテストで E2002 が出るのは、どれも引数が1つの関数なので、指す範囲は変わらない。

## 3. テスト、文書、成功の条件

### 変わるテスト

| テスト | 種類 | 変更 |
|---|---|---|
| `crates/eml_types/tests/check.rs` の `main_with_an_erroneous_row_is_not_reported_again` | 2 | `main : Unit -> <_> Unit` が `main : Unit -> <{error}> Unit` になる |
| `crates/eml_types/tests/check.rs` の `an_undefined_effect_row_is_fresh_at_each_call` | 2 | `run : (Unit -> <_> Unit) -> <_> Unit` と `f#0 : Unit -> <_> Unit` の `<_>` が `<{error}>` になる |
| `crates/eml_types/src/table.rs` の単体テスト (`table/tests.rs` に移す) | 3 | 改名 (`TyShape`、`ArrowLin`、`fresh_*`) と `Tail` への追随だけ。期待値は変えない |

UI テストの出力は変わらない見込みである。上の表にないテストの期待値が変わった場合は、変えずに止まり、差分と理由をユーザーに示して承認を得る。

### 足すテスト

- `crates/eml_types/tests/check.rs`: 未定義のエフェクトを持つ関数が、本体のエフェクトを呼び出し側に伝えないこと。`g : Unit -> <Console> Unit`、`g () = println "x"`、`h : Unit -> Unit`、`h () = g ()` で、E1002 だけが出て E2002 が出ないことを確かめる。
- `crates/eml_types/tests/check.rs`: 引数が2つの関数の E2002 の副ラベルが、本体の row の矢印の部分の型の先頭を指すこと。`f : Int -> Int -> Unit`、`f a b = println "x"` で、副ラベルの位置が2つ目の `Int` (1行目の12列) になることを確かめる。
- `crates/eml_types/src/table/tests.rs` に、次の5つを足す。
  - 末尾が `Error` の row が、閉じた row とも、末尾が rigid な row とも単一化できること
  - 末尾が推論用の変数の row と単一化すると、その変数が末尾 `Error` の row に束縛されること
  - `include_row` が、呼び出し先と今の row のどちらかの末尾が `Error` なら成功すること
  - `copy_type` が末尾 `Error` をそのまま写すこと
  - `solve_kinds` の前に `export` を呼ぶと、debug ビルドで panic すること (`#[should_panic]`)

### 文書

| 文書 | 変更 |
|---|---|
| `docs/implementation/architecture.md` | 「`eml_types` の内部」に、`table/` と `check/` の構成、row の末尾の `Error`、`display` と `export` の使い分けを書く。「ラムダの引数の個数が合わないときや期待する型が壊れているときは、新しい開いた row に退避し」を、末尾が `Error` の row にすると直す |
| `docs/implementation/status.md` | 「次の作業の注意点」から、「診断の食い違い: E2002 の副ラベル…」、「診断の連鎖の残り: …」、「段階3: 未定義のエフェクトの row の末尾は…`σ` を共有…」の3つを消す。「リファクタリング」の表で R2 を R2a (型検査器の内部) と R2b (データモデル) の2行に分け、R2a を完了にする。R2 の一覧を、R2a で済んだものと R2b に残るものに分ける。「完了した作業」に R2a の行を足す |
| `docs/implementation/testing.md` | 「テストの変更の記録」に「リファクタリング R2a」の見出しを作り、上の種類2の変更と、実装の途中で承認を得て変えたテストを記録する |

spec は変えない。`docs/spec/diagnostics.md` の E2002 は、すでに矢印を指すと定めている。`docs/spec/types.md` の「エラーの扱い」の「`Error` が関わる制約からは追加の診断を出さない」は、row の末尾の `Error` にもそのまま当てはまる。

### R2b に残すもの

この回では行わない。R2b の spec で扱う。

- HIR の `Module` の item と名前空間、`Generics`、本体の型の注釈を本体のアリーナに置くこと
- 組み込みの名前、fixity、型、Core IR への変換を1つの表にまとめること、lang item
- エフェクトと型構成子を ID で表すこと (`EffectRef::Io`、`Effect::Io`、`TyCon` をやめる)
- HIR の子の式を辿る関数、パターンが束縛する変数を列挙する関数、ラムダが捕まえる変数を求める関数
- `TypedModule` がスキームを返すこと、テストの表示のためだけの `kinds` を除くこと、`Type::Var` が rigid な変数と解けなかった変数を区別できること
- spec の判断: `x |> f a` の評価順

### 成功の条件

- `table.rs` と `check.rs` が上の構成に分かれ、改名が済んでいる
- `error_rows` がなくなり、未定義のエフェクトの row の末尾は `Error` になる。既知の誤り2件のうち1つ目のテストが通る
- `export` は `solve_kinds` の後にだけ呼ばれ、診断の文言と `check_main` は `display` を使う
- 変わったテストは、上の「変わるテスト」の表のものと、実装の途中で差分を示して承認を得たものだけである
- `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check` が通る
- 上の「文書」の表の変更が済んでいる
