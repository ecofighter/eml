# リファクタリング R4: Core IR のパスの構成の設計

位置づけ: 作業用の設計文書。R4 を終えたら、残す価値のある内容を `docs/spec/`、`docs/implementation/architecture.md`、`docs/implementation/testing.md`、`docs/implementation/status.md` に移し、この文書は削除する。

## 目的と範囲

段階4 (`data`、`match`、タプル) に入る前に、`eml_core_ir` のパスの構成を整理する。段階4では `match` のコンパイルが変換に入り、`simplify` の B2 の作り直しと、Perceus での scrutinee の所有権の扱いも要る。その先には evidence passing の変換や reuse analysis もある。パスが増える前に、次の3つの問題を直す。

1. パスごとに IR を観測できない。`eml_core_ir::lower` が変換、`simplify`、Perceus、verifier を続けて呼ぶので、テストは最後の結果しか見られない。そのため、変換のテストには `simplify` の書き換えが、`simplify` のテストには `dup` / `decref` と `saved` が混ざる。`test-changes.md` に記録したテストの改名や移動は、この結合から出た手間である。
2. パスの前提と保証が、文書のあちこちに散らばっている。「`captures` は Perceus まで古くてよい」「`simplify` は RC の命令のない IR を受け取る」などである。
3. verifier が Perceus の後にしか動かない。`simplify` の誤りも、Perceus を通った後に所有権の誤りとして見つかるので、原因のパスが分かりにくい。

あわせて、940行ある `lower.rs` を役割ごとのファイルに分ける。

言語の観測できる振る舞い (UI テストの出力) は変えない。`lower` の結果 (Perceus の後の IR) も変えない。

### R4 に含めるもの

- パスの順番を1か所に置く `pipeline.rs`、止めるパスを名前で指定する `lower_until`
- パスのあいだで `captures` をつねに正しくする約束
- verifier を、範囲の検査 (`verify_scopes`) と、所有権まで含む検査 (`verify`) に分ける。debug ビルドでは各パスの後に検査をかける
- `lower.rs` を `translate/` のディレクトリに分ける
- Core IR のテストを、確かめるパスごとのファイルに組み替える
- 文書の更新。再帰する join point の項目をロードマップに足すことを含む

### 範囲の外

- join point の引数を `Vec` にすること。段階4で、`match` の共有する枝の join point の形と一緒に決める
- 自分への `jump` とループ化 (contification)。M1 の後、借用パラメータ、evidence passing、ネイティブ化のどれかに着手するときに一緒に入れる (下の「ユーザーと合意済みの決定」)
- パスの trait と、パスの列を回すパスマネージャ
- 前段 (parse、HIR、型検査) の組み直し。線形性の検査パスを分けるのは段階5の仕事である
- `simplify` を不動点まで繰り返すこと

### ユーザーと合意済みの決定

- パスの整理は段階4の spec に含めず、独立した回 R4 として段階4の前に行う。`match` のコンパイルを入れた後に分けるより、入れる前のほうが変更が小さい。
- 再帰する join point は M1 の後に回す。eml のローカルの `let` は再帰せず、再帰はトップレベルの関数だけで、自己末尾呼び出しはすでにフレームを積まない `TailCall` である。そのため、今のインタプリタでループ化の効果は小さい。今の所有の約束では、`jump` が `captures` の RC の対象を1つずつ所有して渡すので、ループの中で変わらない値の `dup` / `decref` も減らない。減らすには、`captures` を借用として扱う Perceus の拡張が要る。
- パスは案A (止めるパスを名前で指定する) で公開する。各パスを公開関数にして呼ぶ側が組み立てる案 (案B) は、パスの順番と前提が公開 API になるので採らない。パスマネージャ (案C) は、パスが3〜4個の今の規模では作り込みすぎなので採らない。
- 既存のテストは、確かめたいパスの直後の IR で見る形に組み替える。新しいテストだけが途中のダンプを使う形にはしない。

## 1. パスの流れと約束

### API

新しく `pipeline.rs` を置き、パスの順番をこのファイルだけが持つ。

```rust
pub enum Pass {
    Translate,
    Simplify,
    Perceus,
}

/// `last` の直後で止める。止めたパスまでの検査は、debug ビルドでかける。
pub fn lower_until(module: &Module, typed: &TypedModule, last: Pass) -> Program;

/// 最後のパスまで進める。R4 の前の `lower` と同じ結果になる。
pub fn lower(module: &Module, typed: &TypedModule) -> Program;
```

- HIR から Core IR への1段目を `translate` と呼び、パス全体を `lower` と呼ぶ。文書の「変換」は `translate` に当たる。
- `eml_cli` と `eml_test_support::core` は、今までどおり `lower` を呼ぶ。

### パスの前提と保証

| パス | 受け取る IR | 渡す IR |
|---|---|---|
| `Translate` | 誤りのない型付き HIR | ANF、join point、末尾呼び出し。RC の命令と `saved` はない |
| `Simplify` | RC の命令のない IR | 同じ形の IR (join point を書き換えた後) |
| `Perceus` | RC の命令のない IR | `dup` / `decref` と `saved` が入った IR |

どのパスの後でも、`captures` と `CoreFn::joins` の索引は正しい。

### `captures` をパスのあいだで正しくする

- パイプラインは、`Translate` と `Simplify` の後に、関数ごとに `liveness::analyze` を呼んで `captures` を埋め直す。パスの中では `captures` が古くなってよい。パスの間でだけ正しくする。
- 理由は2つある。途中で止めたダンプに出る `captures` が正しくなること (古い `[]` が表示されると読む人が誤解する) と、範囲の検査 (2章) がどのパスの後でも `captures` を宣言として扱えることである。
- Perceus は今までどおり最初に `analyze` を呼び、ブロックの入口の生きている変数の表を受け取る。埋め直した `captures` と同じ結果になるので、Perceus の結果は変わらない。
- 解析の回数は関数ごとに1回から3回に増える。どれも「式の数 + ブロックの入口の集合の大きさの和」に比例するので、問題にしない。

### 表示

- Perceus より前の IR には RC の命令も `saved` もない。`pretty` は今と同じく、`saved` が空の呼び出しに何も付けない。表示の関数は変えない。

## 2. verifier の分け方

今の verifier は、範囲の検査と所有権の検査を1回の走査で行う。同じ `Checker` に検査の度合いを持たせて、2つの入口に分ける。

```rust
/// Perceus の後の IR。範囲、引数の数、所有権の釣り合いを確かめる (R4 の前の `verify` と同じ)。
pub fn verify(program: &Program) -> Result<(), VerifyError>;

/// Perceus より前の IR。範囲と引数の数を確かめ、RC の命令がないことを確かめる。
pub fn verify_scopes(program: &Program) -> Result<(), VerifyError>;
```

### `verify_scopes` が確かめること

- `verify` と共通の検査。変数を2回束縛しないこと、変数と join point を範囲の外で使わないこと、`Join` を2回定義しないこと、`joins` の索引が `Join` を指すこと、`Join` の時点で `captures` が範囲にあって昇順であること、join point の本体が `captures` と引数だけの範囲から始まること、`Switch` の枝のタグが重ならないこと、`jump` の時点で行き先の `captures` が範囲にあること、直接呼び出しとクロージャの引数の数、エフェクトの表。
- `verify_scopes` だけの検査。`Dup`、`Decref`、空でない `saved` が現れないこと。Perceus より前のパスが RC の命令を作らないという約束を、ここで確かめる。

### `verify_scopes` が確かめないこと

- 所有の数 (`State::owned`) を数えない。
- 呼び出しの後に範囲を区切り直さない。区切り直しの規則は「呼び出しの後は `saved` の変数と結果の変数だけが範囲にある」で、`saved` を前提にしているためである。

### パイプラインでの検査

- debug ビルドでは、`Translate` と `Simplify` の後に、`captures` を埋め直してから `verify_scopes` をかける。`Perceus` の後は `verify` をかける。
- `lower_until` で途中で止めたときも、止めたパスまでの検査はかける。
- panic の文言にパスの名前を入れる。`internal error: invalid Core IR after simplify: {error}` のようにする。パスの名前は `translate`、`simplify`、`perceus` とする。

## 3. `lower.rs` の分け方

`lower.rs` を `translate/` のディレクトリに移し、役割ごとにファイルを分ける。関数の中身は変えず、置き場所と可視性だけを変える。

| ファイル | 中身 |
|---|---|
| `pipeline.rs` | `Pass`、`lower_until`、`lower`、`captures` の埋め直し、各パスの後の検査 |
| `translate/mod.rs` | `translate` (関数ごとの変換、入口の関数、`Program` の組み立て)、`FnLowering`、`lower` と `lift`、`Binding` と `Exit` と `exit_with`、`new_var`、`push`、`seq`、`tail`、`tail_expr`、`stmts` |
| `translate/expr.rs` | `atom`、`ty`、`bind`、`call_known`、`call_builtin`、`call_operation`、`call_args`、`bind_pat` |
| `translate/program.rs` | `ProgramBuilder`、`Strings`、組み込みと操作を包む関数、入口の関数、`effect_index`、`perform_call`、`effect_table` |
| `translate/types.rs` | `boxed`、`var_info`、`split_arrows`、`Lowering` と `lowering` |

- `translate/mod.rs` には、式の値の渡し先と join point の組み立てという、制御の骨組みだけを残す。段階4の `match` のコンパイルは `translate/pattern.rs` に置き、決定木の枝の join point も、この骨組み (`Exit::Jump` と `Binding::Join`) で作る想定である。
- 表の割り当ては目安である。実装で、ある関数を別のファイルに置いたほうが可視性が少なくて済むと分かったら、そちらに置いてよい。
- `simplify.rs`、`perceus.rs`、`liveness.rs`、`pretty.rs` は動かさない。
- `lib.rs` の `CExpr::Join::captures` のコメント (「Perceus の最初の解析が埋め直すので、変換や `simplify` は空のままでよい」) を、1章の約束に合わせて直す。

## 4. テスト

### `eml_test_support`

- `core_until(text, Pass)` を足す。誤りのないソースを `lower_until` で指定したパスまで進める。
- `core(text)` は今のまま、`lower` の結果を返す。
- `tests/support.rs` に、`core_until` が指定したパスで止まることを確かめるテストを1件足す。`Translate` で止めた IR に `dup` / `decref` がなく、`Perceus` で止めた IR が `core` と同じ表示になることを確かめる。

### Core IR の既存のテストの振り分け

`tests/common/` に、パスを指定して表示する補助関数を置く (今の `core_text` に止めるパスの引数を足すか、パスごとの関数にするかは計画で決める)。

| 移す先 | 止めるパス | テスト | 種類 |
|---|---|---|---|
| `tests/perceus.rs` (新設) | `Perceus` | `strings_are_dupped_and_decreffed`、`shadowed_and_discarded_strings`、`a_non_tail_if_keeps_strings_used_later`、`calls_save_the_variables_used_after_them` | 1 (移動のみ。期待値は変わらない) |
| `tests/translate.rs` (`lower.rs` を改名) | `Translate` | `hello_world`、`recursion_and_top_level_values`、`partial_and_extra_arguments_use_closures`、`builtins_used_as_values_are_wrapped`、`lambdas_are_lifted_with_their_captures_first`、`a_zero_arity_callee_is_evaluated_before_its_arguments`、`a_tail_if_returns_from_each_arm`、`calls_in_tail_position_are_tail_calls`、`the_entry_applies_a_point_free_main_to_unit`、`nested_join_points_capture_what_outer_join_points_need`、`handlers_are_lifted_to_closures`、`operations_as_values_and_drop` | 1 (ファイルの改名) と 2 (期待値) |
| `tests/simplify.rs` | `Simplify` | 今の7件 | 2 (期待値) |

- `perceus.rs` に移す4件は、どれも `dup` / `decref` の位置か `saved` の並びを確かめるテストである。止めるパスは `core` と同じ最後のパスなので、期待値は1文字も変わらない。
- `translate.rs` の期待値からは、`dup` / `decref`、`saved`、`simplify` の書き換えが消える。`simplify.rs` の期待値からは、`dup` / `decref` と `saved` が消える。
- `nested_join_points_capture_what_outer_join_points_need` は、`captures` を確かめるテストなので `Translate` で止める。1章の約束によって `Translate` の直後にも正しい `captures` が出る。`simplify` の前の形のほうが、テストの名前どおりの入れ子が残る。
- 種類2の期待値は、スナップショットを取り直した後に差分を読み、消えたのが RC の命令、`saved`、`simplify` の書き換えだけであることを確かめる。それ以外の行が変わっていたら、R4 の誤りとして扱う。`simplify` が書き換えない join point の `captures` は、R4 の前の期待値と同じになるはずで、変わっていたらそれも誤りとして扱う。

### 足すテスト

`tests/verify.rs` に、手書きの IR (`eml_test_support::ir`) で次のテストを足す。

- RC の命令のない IR を `verify_scopes` が受け入れる。
- `Dup` を含む IR と、空でない `saved` を含む IR を、`verify_scopes` が拒む。
- 範囲の外の変数の使用を、`verify_scopes` が拒む。

### 変えないテスト

- `tests/verify.rs` の既存のテスト、`eml_interp` のテスト、UI テスト、CLI テスト。

### 記録

- 種類1と種類2の変更は、`docs/implementation/test-changes.md` の R4 の節に記録する。

## 5. 文書

| 文書 | 更新する内容 |
|---|---|
| `docs/spec/core-ir.md` | パスの表 (1章の前提と保証)。「`captures` は変換が空のままでよい」を「パスのあいだではつねに正しい」に改める。verifier の2つの度合いと、debug ビルドで各パスの後に検査をかけること |
| `docs/implementation/architecture.md` | `pipeline.rs` と `translate/` の構成、`verify_scopes`。`lower.rs` を指している箇所の書き換え |
| `docs/implementation/testing.md` | Core IR のテストをパスごとのファイル (`translate.rs`、`simplify.rs`、`perceus.rs`、`verify.rs`) に置くこと。`core` と `core_until` の使い分け。`eml_test_support` の関数の一覧に `core_until` を足す |
| `docs/implementation/status.md` | リファクタリングの表に R4 を足し、前書きの「7つの回」と順番の記述を直す。完了した作業と `eml_core_ir` の状況を更新する。次の作業の注意点に、段階4で `match` の共有する枝のために join point の引数を `Vec` にすることを足す |
| `docs/future/roadmap.md` | 「処理系」に、再帰する join point (自分への `jump`、ループ化、contification) の項目を足す。入れる時期と理由 (合意済みの決定のとおり)、残りの作業 (引数の `Vec` 化は段階4で済む予定、verifier の「自分へ jump しない」制約を外す、変換にループ化を足す)、互いに jump し合う join point は別に設計すること、生存解析には不動点の計算が要らないこと |
| `docs/implementation/test-changes.md` | R4 の節 (4章の種類1と種類2) |
| `CLAUDE.md` | `eml_test_support` の関数の一覧に `core_until` を足す |

## 成功の条件

- `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check` が通る。
- `lower` の結果が R4 の前と同じである。`tests/perceus.rs` の4件の期待値と、UI テストの出力が変わっていないことで確かめる。
- debug ビルドで、すべてのテストの `lower` が各パスの後の検査を通る。
- `translate.rs` と `simplify.rs` の期待値の差分が、4章で挙げた行だけである。
- 文書が5章のとおりに更新されている。
