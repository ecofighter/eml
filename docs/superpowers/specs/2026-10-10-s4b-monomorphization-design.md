# S4b 単相化

位置づけ: 作業の設計。ロードマップの S4b (単相化) を定める。S4b を終えたら、決まったことを `docs/spec/core-ir.md` と `docs/implementation/` に移し、この文書を削除する。

## 目的

関数のコードを、入口から届く (関数, 型引数) の組ごとに変換する。型変数の位置を通っていた `Int` などのスカラーが、具体化した型の Repr で渡るようになる。S5 の型クラスの証拠も、この instance の表で決める。

`box` と `unbox` の回数は、型変数の位置を通る値が減るぶん減ることが多いが、減る量は約束しない。CPS のラムダの位置の変換は単相化では消えず ([ロードマップ](../../future/roadmap.md) の「段の列」の順序の理由)、`pair x = (x, x)` を `Int` で使うと、呼ぶ側で1回だった `box` が `pair@[Int]` の中で2回になる形もある。前後の回数は `bench/` で記録する。

次のものは変えない。

- 表面の構文
- データの配置。型変数のフィールドは `tobj` のままで、`Option Int` と `Option a` は同じ配置を使う ([Core IR とインタプリタ](../../spec/core-ir.md) の「データの配置」)
- row。row の多相は S7 の evidence passing で扱う
- UI テストの出力

## 前提として確かめたこと

- translate は型から Repr を決めるとき、`Con` の型引数を見ない (`List Int` も `List a` も `obj`)。代入が Repr を変えるのは、型が型変数そのもののときだけである。ただし、呼び出し先の instance の鍵には、代入した後の型 (`List a` に `a := Int` を代入した `List Int`) が要る
- 型の表 (`TypeStore`) の公開の API は読むものだけで、代入の関数はない。型を足す `intern` は `eml_types` の中からしか呼べない。`TypeStore` は `Clone` を持つ
- 参照ごとの具体化の表 (`BodyTypes::instantiations`) は、本体の中のトップレベルの item への参照ごとに、宣言と型引数を持つ。型引数はシグネチャの型変数の順 (`generics.type_vars` の順、`Shape::rigids` の順と同じ) に並び、row にだけ現れる型変数はない。関数の本体の表には、その本体の中のラムダ、handle の本体、節の中の参照も入る。関数を値として使う参照も `ExprKind::Path` なので記録される
- シグネチャの型変数の名前は、1つのシグネチャの中で重ならない
- 関数の本体の型に現れる rigid な型変数は、その関数自身のシグネチャの型変数 (`Shape::instantiate_rigid`) と、handler の節の操作ごとの型変数 (`Shape::instantiate_with_effect_args`) の2種類だけである。型の注釈はシグネチャの型変数しか指せず、局所的な `let` とラムダは単相で、操作とコンストラクタの参照は毎回新しい変数で具体化する
- 型の表は rigid な型変数を名前だけで登録する。そのため、関数の型変数 `a` と、handler の節の操作ごとの型変数 `a` が同じ `TypeId` になる (`crates/eml_types/tests/instantiations.rs` の `a_clause_variable_is_shown_like_the_function_variable_of_the_same_name`)
- eml の型は rank-1 で、トップレベルの関数はシグネチャが必須である。そのため、instance が増えるのは名前の参照からだけで、その参照はすべて `instantiations` に記録されている
- 多相再帰 (`depth : Int -> a -> Int` の本体が `depth (n - 1) (x, x)` と呼ぶ形) は、今の型検査を通り、実行できる
- 型変数の値への `==` は E2006 になる。`==` と `!=` の型引数は、いつも `Int`、`String`、`Bool` である
- UI テストに多相再帰はない。UI テストの出力 (実行時エラーの位置を含む) に、関数の名前は現れない
- 型の表示 (`TypeStore::display`) は型を木としてたどる。部分を共有する型では、表示の長さが型の表の大きさの指数になる ([実装の現在地](../../implementation/status.md) の「深さと性能」)
- box の挿入、縮約、Perceus は型を読まず、Core IR の関数ごとに動く

## `eml_types` の変更

### 操作ごとの型変数 `OpVar`

- `TypeKind` に `OpVar(String)` を足す。handler の節で、操作ごとに量化した型変数を具体化した変数 (`Shape::instantiate_with_effect_args` が作る rigid な変数) は、書き出すときに `OpVar` にする。推論の表の rigid な変数に、操作ごとの変数かどうかの印を持たせ、書き出し (`table/export.rs`) がその印を見て種類を選ぶ
- `OpVar` は節の中だけに現れるとは限らない。節の値が handle の結果になると (`put x k -> drop k; id x`)、handle の外の本体の型にも現れる。どこに現れても、扱いは下の代入の規則のとおりである
- 関数の本体の型と、関数の宣言の型に現れる `Rigid` は、その関数自身のシグネチャの型変数だけになる。操作とコンストラクタの宣言の型 (`op$`、`con$`、データの配置が読むスキーム) の `Rigid` は、今のまま残る。代入は instance 自身の関数の型にだけかけ、呼ぶ先の関数、操作、コンストラクタの宣言の型にはかけない
- 表示は今と同じ名前のままにする。診断の文言と `instantiations.rs` のスナップショットは変わらない
- 推論 (単一化、Kind、持ち越し) は推論の表の上で動くので、変わらない
- row の rigid な末尾 (`RowTail::Rigid`) は変えない。row は単相化しない

### 代入の API

- 代入を表す値 `Substitution` を `eml_types` に足す。シグネチャの型変数の名前から型への対応と、代入した結果を覚える表を持つ。`Substitution::new(vars: Vec<(String, TypeId)>)` で作る
- `TypeStore` に公開の関数 `substitute(&mut self, ty: TypeId, subst: &mut Substitution) -> TypeId` を足す
  - `Rigid(名前)` を、対応のその名前の型に置き換える。対応にない名前の `Rigid` はそのまま残す
  - `OpVar` は、いつも `Flexible` に置き換える。節の型変数は一様に扱うので、関数の型引数と名前が同じでも置き換えない。この結果、translate は `OpVar` を見ない
  - 関数型の中のエフェクトの型引数 (`<State a>` の `a`) と、タプルの要素にも代入する。row の末尾には触れない
  - 結果は `intern` で登録するので、同じ型は同じ `TypeId` になる
  - 同じ `Substitution` で代入した型は、`Substitution` の表で覚える。1回の呼び出しの中でも、共有された部分はこの表で1回だけたどる。そのため、1つの `Substitution` での代入の費用の合計は、たどった型の表の節点の数に比例する
  - 型の表は、型ごとに「型変数 (`Rigid` か `OpVar`) を含むか」の印を `intern` のときに計算して持つ (今の `contains_error` の印と同じ形)。印のない型は、たどらずにそのまま返す
- 表示に長さの上限を付けた関数 `display_bounded(ty, names, limit) -> Option<String>` を足す。表示が `limit` 文字を超えた時点でたどるのをやめ、`None` を返す。費用は `limit` に比例する
- 型を作るのは今と同じく `intern` だけで、`intern` は `eml_types` の中に閉じたままにする。後の段階が型の表に型を足せるのは、`substitute` を通したときだけである。`TypeStore` の決まり (「後の段階は表を読むだけで、型を作らない」) を、そのように書き換える
- translate は、始めに `typed.types` を複製して自分で持ち、代入の結果をその複製に足す。`eml_core_ir::lower` のシグネチャ (`&TypedProgram` を受ける) は変えない

## translate の instance の表

### 鍵

- instance の鍵は (関数, 型引数の列) である。型引数は型の表の `TypeId` で、シグネチャの型変数の順に並ぶ。型変数を持たない関数の鍵は、空の列である。row だけについて多相な関数も、鍵は空の列である
- 鍵を作るときは、参照の型引数に、参照を含む instance の `Substitution` をかける。代入の後に残る `Flexible` (節の型変数から来たもの、型検査で最後まで決まらなかったもの) は、そのまま鍵に入る
- 鍵は型そのものなので、`List Int` と `List String` のように Repr が同じでも、別の instance になる。S5 で証拠が型ごとに変わるためである。型引数の中の関数型の row (`Unit -> <e> Int` と `Unit -> <IO> Int`) が違う場合も、別の instance になる。Repr が同じ instance をまとめるのは、後の処理系の最適化に回す (ロードマップの「処理系」に足す)

### 一様な位置と多相再帰

- 多相再帰は、変換の前に型変数の流れのグラフで見つける
  - 節点は、(関数, 型変数の番号) である。HIR のすべての定義された関数について作り、届くかどうかは見ない
  - 関数 f の本体の参照 `g @[T0, T1, …]` で、f の型変数 i が Tj に現れるとき、(f, i) から (g, j) へ辺を引く。「現れる」は、`substitute` がたどる位置 (型構成子の引数、関数型の引数と結果とエフェクトの型引数、タプルの要素) のどこかにあることである。Tj が `Rigid(i の名前)` そのものでなければ、その辺に「大きくなる」の印を付ける
  - 強連結成分は、大きくなる辺の両端を含むとき「大きくなる成分」である。自分自身への大きくなる辺を持つ1つの節点 (`depth` の型変数) も、大きくなる成分である
  - 大きくなる成分の節点を、一様な位置とする
  - 強連結成分は、プログラムの大きさに比例して Rust のスタックを使わない形 (作業の列) で求める
- 鍵を作るときは、呼ぶ先の一様な位置の型引数を、参照の型引数によらず `Flexible` にする。ほかの位置は、上の規則のとおり具体化する
- 一様な位置は、関数ごとでなく型変数の位置ごとに決める。S5 は、証拠が循環に沿って大きくなる制約付きの多相再帰だけを型検査で弾く。大きくならない型変数の制約の証拠は、この位置ごとの決め方で具体的な型から確定できる
- 一様な位置の型引数は `Flexible` なので、大きくなる成分を通る値の型の大きさは有界になる。大きくなる成分の外の位置は、流れ込む型が有界な型と `Flexible` から作られるので、有界である。そのため、instance を集める手順は止まる
- 一様な版 (すべての位置が `Flexible` の instance) のために、別の経路は作らない。`Flexible` は今も `tobj` になる

### ロードマップからの変更

ロードマップの S4b の「決めたこと」は、「多相再帰で具体化が上限を超えたときは、型変数の位置を `tobj` のまま扱う一様な版を使う」としていた。この spec は上限で打ち切らず、上のグラフで多相再帰を前もって見つけ、一様にするのも型変数の位置ごとにする。理由は次のとおりである。

- 結果が変換の順によらず、上限までの無駄な instance も作らない
- 位置ごとに決めると、大きくならない型変数の証拠を S5 で確定できる

instance の総数の上限も置かない。多相再帰がなくても、型を倍々に大きくして呼ぶ関数の鎖を書けば、instance の数を指数的に増やせる。この形は、既知の制限として `docs/implementation/status.md` に書く。

### instance を集めて番号を振る

`crates/eml_core_ir/src/translate/instances.rs` を新しく作り、本体を変換する前に次を行う。

1. 多相再帰を見つけ、一様な位置の集合を作る
2. 入口の instance から始め、届く instance をすべて集める
   - 入口の関数の鍵は、型変数を持たなければ空の列、持てばすべて `Flexible` である。入口は `main` とは限らない (`lower` の呼び出し側が選ぶ。S11 の REPL もこの経路を使う)
   - 見つけた instance を、先に見つけたものから順に処理する作業の列 (FIFO) で処理する
   - 各 instance の本体の `instantiations` を、式の ID の順にたどる。関数への参照 (呼び出しと、値としての参照の両方) ごとに、上の規則で鍵を作り、まだない鍵なら列の後ろに足す。extern の関数、操作、コンストラクタへの参照は instance を作らない
3. 各 instance の本体の型の表 (`exprs`、`locals`、`pats`、`instantiations` の型引数) に代入をかけた表を作る。代入が空の instance (型変数を持たない関数) は、元の表をそのまま使い、写さない。`masks` は型を持たないので、元のまま使う。debug ビルドでは、代入をかけた表に `Rigid` と `OpVar` が残っていないことを確かめる
4. instance を、HIR の関数の順 (`hir.functions()` の順) に並べ、同じ関数の instance は 2 で見つけた順に並べる。その順に `FnIdx` を予約する。`TypeId` の値の順には並べない。後の段階が `TypeId` の値に意味を持たせないためである。型変数を持たない関数しかないプログラムでは、関数の並びが今と同じになる

この手順が、今の `reachable` (関数の集合) を置き換える。番号がすべて決まってから本体を変換するので、変換の途中で instance の番号を予約することはない。代入をすべて 3 で済ませるので、本体の変換は、複製した型の表を読むだけである。

### instance の本体の変換

- 関数から番号への表 `ItemMap<Function, FnIdx>` を、鍵から番号への表に替える。呼び出しと値の参照 (`translate/expr.rs` の `Callee::Function` と関数値の参照) は、代入をかけた `instantiations` から鍵を作って引く
- 本体の文脈 (`BodyCtx`) は、元の本体の型の表の代わりに、代入をかけた表を指す。`BodyCtx` は今と同じく `Copy` で、型の表を読むだけである
- トップレベルの関数の引数と `ret` の Repr は、宣言の型に instance の代入をかけた型から決める。この型も 3 で作っておく
- `==` と `!=` の比べ方は、代入をかけた `instantiations` の型引数から決める
- ラムダ、handle の本体、節は、外側の instance の表をそのまま使う。節の型変数は `Flexible` になるので、節の中のコードは一様なままである
- `op$`、`con$`、`cont$`、`cont$state` と、データの配置は、今と同じくスキームから作る。`$externN` の包む関数も今と同じである

### 名前

- 型変数を持つ関数の instance の名前は、元の名前 (モジュールの修飾を含む) に `@[型引数, …]` を付けた形にする。例は `map@[Int, String]`、`Prelude.>>@[Int, Int, Int]`、`length@[List Int]` である。一様な位置 (`Flexible`) は `_` と書く (`depth@[Int, _]`)
- 型引数は、診断と同じ表示で書く。型の名前は、診断と同じく、名前が重なるときだけモジュールで修飾する。データの配置の名前 (いつもモジュールで修飾する) とは規則が違う
- 型引数の表示は `display_bounded` で作り、`@[` と `]` の間が 64 文字を超える instance は、`@[…]` の代わりに、その関数の instance の中の順番 (上の 4 の順、1 から数える) を `@` の後に付ける (`ident@3`)。部分を共有する型 (`let f1 = f0 ident` の列の `ident` の型引数) で名前が指数の長さになるのを防ぐためである。`@[` で始まる名前と `@` と数字の名前は重ならない
- 同じ関数の2つの instance の `@[…]` の名前が重なったときは、後の instance (上の 4 の順) を順番の名前にする。異なる型が同じ表示になる場合 (row の末尾の表示など) があっても、名前は重ならない
- 型変数を持たない関数の名前は、今と同じである
- ラムダ、handle の本体、節、`$externN` の名前は、instance の名前を根にする (`map@[Int, String]$lambda0`)。`f$boxed` も instance の名前に付ける (`f@[Int]$boxed`)
- 実行時エラーのうち位置を持たないもの (`{fault} in \`関数の名前\``) は、instance の名前を出す。UI テストの出力にこの形はない

### テキストの形

- 今の lexer は、空白、`( ) { } [ ] ,`、`"` で語を切る
- `pretty` は、関数の名前が空のとき、または空白、`(`、`)`、`{`、`}`、`[`、`]`、`,`、`"` のどれかを含むとき、名前を文字列と同じ逃がし方 (Rust の `{:?}`) で引用符で囲んで書く (`fn "map@[Int, String]"(…)`)。関数の参照は `&"map@[Int, String]"` と書く (`&` の後に文字列の字句が続く)
- `parse` は、関数の名前が来るすべての位置 (`fn` の見出し、`call`、`tail`、`closure`、`&`) で、語の代わりに文字列も受け付ける。`apply`、`handle`、節のオペランドの関数の参照も `&` の規則に従う

### `f$boxed` の規則の根拠

`docs/spec/core-ir.md` の「一様な関数」の、「ネイティブ化では標準ライブラリを一度だけ翻訳して使い回せなくなる」を書き直す。総称な関数は単相化でプログラムごとに変換するので、使い回せるのは型変数を持たない関数と、同じ鍵の instance である。トップレベルの関数の ABI を、その定義 (と instance の型引数) と、それが末尾の位置で呼ぶ関数だけで決める、という規則は変えない。定義ごとに Core IR を保存する REPL で、古い定義を書き換えずに済む、という理由も残る。

## テスト

### 新しいテスト

- `crates/eml_types/tests/`
  - 代入: 入れ子の型、関数型のエフェクトの型引数、タプル、`OpVar` が `Flexible` になること、対応にない `Rigid` が残ること、同じ代入の結果が同じ `TypeId` になること、型変数を含まない型がそのまま返ること
  - 書き出し: 節の型変数が `OpVar` になり、関数の同じ名前の型変数と別の `TypeId` になること
  - `display_bounded`: 上限の内では `display` と同じ文字列になり、上限を超えると `None` になること
- `crates/eml_core_ir/tests/instances.rs` (`tests/main.rs` に登録し、ふだんの `cargo test` で流す)。instance を数えるときは、`Pass::Translate` までの Core IR で、内部の印のない関数のうち、名前が `関数名@` で始まるものを数える
  - 多相な関数の鎖 (各 `fi : a -> a` が `f(i+1)` を呼び、`fN` まで) を `main` が `Int` と `String` で使うと、鎖の instance がちょうど 2N 個になる
  - 1つの多相な関数を K 種類の型で使うと、その関数の instance が K 個になる
  - 多相再帰 (`depth : Int -> a -> Int`) は `depth@[_]` の1つだけになる。相互の多相再帰 (f が g を `(a, a)` で呼び、g が f を `a` で呼ぶ) も、両方の位置が一様になる
  - 位置ごとの一様: `walk : Int -> a -> b -> b` が `a` だけを大きくして自分を呼ぶとき、`walk` を `b := Int` で使うと `walk@[_, Int]` になり、`b` の位置の引数と `ret` の Repr が `int` になる
  - 節の中で総称な関数を節の型変数で呼ぶと、その instance の鍵の位置が `Flexible` になる。節の値が handle の結果として外に出て、外で総称な関数に渡されるときも同じになる
  - 部分を共有する型 (`let f1 = f0 ident` の列、長さ 64) を総称な関数の本体に置いて `main` から使うと、名前が `ident@N` の形になり、`lower` が終わる
  - 型変数を持たない関数だけのプログラムは、関数の並びと名前が今と同じになる
  - 入口に型変数を持つ関数を選ぶと、その鍵がすべて `Flexible` になる
- 同じファイルに、コンパイル時間のテストを置く。`eml_core_ir::lower` の時間だけを測り、各大きさで3回測った最小を使う。形は2つで、大きさ 2000 と 8000 の時間の比が6以下なら通る
  - 多相な関数の鎖 (上と同じ形で、`Int` と `String` で使う)
  - 部分を共有する型を作る `let` の列 (`crates/eml_types/tests/scaling.rs` と同じ形) を、総称な関数の本体に置いて `main` から使う形。型が深いので、`eml_types` と同じく 64 MiB のスタックのスレッドで測る

  `crates/eml_types/tests/scaling.rs` に合わせて `#[ignore]` を付け、release ビルドで流す
- `crates/eml_core_ir/tests/translate.rs`: `id` を `Int` と `String` で使うと2つの instance になり、引数の Repr がそれぞれ `int` と `obj` になること、ラムダと節の名前が instance の名前を根にすることを、スナップショットで確かめる
- `crates/eml_core_ir/tests/text.rs`: 引用符で囲んだ名前が、`fn` の見出し、`call`、`tail`、`closure`、`&` で読み戻せること
- UI テスト `tests/ui/run/functions/polymorphic_recursion.em`: 多相再帰と位置ごとの一様が最後まで動くことを、`debug_heap` 付きで確かめる

### テストの変更

[テスト戦略](../../implementation/testing.md) の「テストの変更の運用」の種類で書く。

- 成否の変更
  - boxing.rs の `int_constants_are_boxed_and_other_constants_pass_as_they_are` と `a_unit_value_passes_to_tobj_without_an_instruction` は、多相な関数の引数で `box` を出していた。単相化でその `box` が消え、今のプログラムでは規則を確かめられなくなる。そこで、同じ名前のまま、データの型変数のフィールドで同じ規則 (`int` の定数は `box` を通り、`unit` の値は命令なしで `tobj` に渡る) を確かめるプログラムに置き換える
- 期待値の変更 (理由はどれも、関数のコードを instance ごとに変換するようになったためである)
  - translate.rs のうち、型変数を持つ関数を具体的な型で使う次のテストの、関数の名前、Repr、`box` と `unbox` を書き換える: `each_reference_to_an_extern_as_a_value_gets_its_own_wrapper`、`names_outside_the_entry_are_qualified_with_their_module`、`builtins_used_as_values_are_wrapped`、`lambdas_are_lifted_with_their_captures_first`、`externs_are_called_by_their_canonical_name` (`contains` で調べる文字列)、`a_masked_callback_call`、`a_saturated_known_call_takes_the_mask_of_its_last_arrow`、`extra_arguments_of_a_known_call_take_the_mask_of_their_arrow`、`a_continuation_call_in_its_clause_has_no_mask` (`function` に渡す名前を含む)
  - perceus.rs の `a_nested_pattern_gives_up_the_parent_before_the_release`
  - 上に挙げた以外に、計画を作るときに型変数を持つ関数を名前で引くテストが見つかれば、同じ種類の変更として扱う。範囲は「`eml_core_ir` のテストのうち、型変数を持つ関数を具体的な型で使うプログラムのダンプ、`function` に渡す名前、`contains` で調べる文字列」である
  - `crates/eml_interp/tests/bench.rs` のスナップショットの回数を書き換える。この差分が、S4b の前後の回数の記録である
- 機械的な追随
  - `crates/eml_core_ir/tests/common/mod.rs` の `function` が、引用符で囲んだ名前の関数も見つけられるようにする
  - `crates/eml_core_ir/tests/externs.rs` の `has_type_var` の網羅的な `match` に、`OpVar` の腕を足す
  - `instantiations.rs` の、区別を S4b に送っていたテストのコメントを直す。そのテストのスナップショットは変わらない
- UI テストの出力は変えない

## 文書

- `docs/spec/core-ir.md`
  - 「変換の規則」に、instance、鍵、一様な位置と多相再帰の検出、instance を集める順、名前を書く。「変換は、入口の関数から届く関数だけを Core IR にする」を、届く instance の説明に替える
  - 「位置の規則」の「トップレベルの関数と `op$`、`con$`、`$externN` はスキームから」を、トップレベルの関数は instance の代入をかけた宣言の型から、と直す
  - 「一様な関数」の `f$boxed` の根拠を、上のとおり書き直す
- `docs/implementation/testing.md`: 「Core IR のテキストの形」に引用符で囲む名前を、「性能のテスト」に instances.rs を足す。段階の API を直接呼ぶテストの列 (「crate の中の置き方」) に、instances.rs のコンパイル時間のテストを足す
- `docs/implementation/architecture.md`
  - 「`eml_types` の内部」に、`OpVar`、`Substitution` と `substitute`、型変数を含むかの印、`display_bounded`、型の表の決まりの変更を書く。名前の衝突の注意書きを消し、「S4b の単相化」のような先の段への言及を、今の仕組みの説明に直す
  - 「translate の組み立て」に、instance を集める手順、代入をかけた本体の型の表、鍵から番号への表を書く。持ち上げた関数の名前の説明に、instance の名前を根にすることを足す
- `docs/implementation/status.md`: 「深さと性能」に、instance の数が指数的に増える形と、Repr が同じでもできる複製を書く
- `docs/implementation/benchmarks.md`: S4b の記録を足す。冒頭と `list.em` の説明の、S4b を先の段として書いた文を直す
- `docs/overview.md`: 「確定した設計判断」に、関数のコードは単相化し、データの配置は一様のままにすることを足す
- ロードマップ
  - 段の列から S4b の行を消し、S5 と S7 の前提を「なし」にする。S4b の節を消す
  - 冒頭の段落の「S4b〜S13」を「S5〜S13」にする
  - 順序の理由の、S4b を主語にした2つの項目 (先頭に置く理由と、減る量を約束しない理由) を、終えた段として読める形に直す
  - S5 の節の「S4b の instance の表」などの参照と、S9 の節の「(S4b)」は、`core-ir.md` の該当する節を指すように直す
  - 「処理系」の最適化パスに、Repr が同じ instance をまとめることを足す
- `docs/README.md` の表の「再設計の段 (S4b〜S13)」を「(S5〜S13)」にする
- CLAUDE.md の、Core IR は型を読むだけという記述 (`TypeStore::intern` is crate-private, so Core IR only reads types) を、代入でだけ型を足す形に直し、translate の説明に instance を足す

## ロードマップの論点の扱い

- 具体化の上限の値: 上限は置かない (上の「ロードマップからの変更」)
- instance の名前の付け方: 上の「名前」
- instance の複製の数を報告するか: 報告しない。数は instances.rs のテストと `bench/` の回数で見える
- 関数の型変数と、同じ名前の handler の節の型変数の区別: 上の「操作ごとの型変数 `OpVar`」

## 完了の条件

- `cargo test` がすべて通り、UI テストの出力が変わらない
- instances.rs の数のテストが通り、コンパイル時間のテスト (release、`--ignored`) が比6以下を守る
- `bench.rs` のスナップショットの差分と、`bench/run.sh` の S4b の記録が `benchmarks.md` にある
- `cargo clippy --all-targets`、`cargo fmt --check`、`nix build` が通る

## 進め方

worktree のブランチで実装し、main へ fast-forward で入れる。入れたら、この spec と S4b の計画を削除する。
