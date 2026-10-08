# S3b-2c-1 Core IR v2 の表 (設計)

位置づけ: 作業の設計。再設計のサブプロジェクト S3b-2c-1 の spec である。全体の方針は [ロードマップ](../../future/roadmap.md) の「S3b-2c Core IR v2 の表現」と、[全体設計](2026-10-07-redesign-design.md) にある。決まったことは S3b-2c-1 の終わりに `docs/` の該当文書へ移し、この文書を削除する。

## 目的

Core IR に、データの配置の表と extern の表の Repr を持たせる。`con`、タグの `switch`、`unpack`、`release` は配置の ID でコンストラクタを指し、verifier はそれぞれの命令を、その命令が指す配置と extern の行に照らして確かめる。値の意味は変えない。

この表は、次の段 S3b-2c-2 で box と unbox を入れるパスが読む。後のバイトコード VM ではジャンプ表の大きさとポインタのフィールドを、ネイティブ化では物体の記述子を、reuse では同じ配置かどうかを、この表から知る。

## 段の分け方

ロードマップの S3b-2c を2つの段に分ける。

- S3b-2c-1 「表」 (この文書): extern の表の Repr、データの配置の表と配置の ID、今の IR で成り立つ verifier の検査。意味は変えない
- S3b-2c-2 「境界」: `box` と `unbox`、それを入れるパス、多相な位置の Repr の規則、呼び出しの結果と `ret` の比較、末尾呼び出しを作る場所

分ける理由は2つある。-2 は -1 が作る配置のフィールドの Repr と extern の行を読む。また、期待値の変わる範囲が、-1 ではすべての `con`、`switch`、`unpack`、`release` のテキスト、-2 では Repr の境界と `tail` の位置になり、段ごとに1種類にまとまる。

次の3つは S3b-2c から外す。

- `Float` の Repr と `Value::Float`: 型とリテラルが入る「`Float`、`Char`、`Num`」の段に回す。今入れると、テストできない死んだコードになる
- フィールドのない `#N` の行き先で scrutinee を所有しない規則: IR の規則にはしない。VM とネイティブのバックエンドが、`switch` の配置の ID を見て、その行き先の `decref` をその場で消す。IR の規則にすると、verifier に「即値と分かっている」という3つ目の状態が要る
- 所有の都合による `TailCall` の降格: S3b-2b で、フィールドは呼び出しより前にすべて所有になったので、借用パラメータを入れるまで起きない。借用パラメータと一緒に入れる

## 背景

- `Repr` は `translate/types.rs` の1か所で決まる。型の引数を見ないので、`Option Int` と `Option a` はどちらも `tobj` である。extern の型の Repr はそこにハードコードしてある
- `con`、`switch`、`unpack`、`release` はタグの番号だけを持ち、どの型のコンストラクタかを持たない。translate は命令を出す所でコンストラクタか型を知っているのに、捨てている。そのため verifier は、`con` と `switch` が同じ型か、case のフィールドの数が合うか、タグが範囲にあるかを確かめられない
- extern の表は Repr を持たない。型の行の `heap` はどこからも読まれていない。関数の行は名前と引数の数と純粋さだけを持つ
- UI の run と run-fail の 115 本で、extern の呼び出し 801 か所の Repr は、すでに std のシグネチャの Repr と合っている

この spec の規則をすべて実装した試作では、次のことを確かめた。

- 今の translate、contract、Perceus の出力は、UI のプログラムでもテストの IR でも、新しい検査をすべて満たした
- 成否の変わるテストはなかった。UI テストの出力は変わらなかった
- translate の `single()` が1つの case の `switch` を出す分岐は、一度も通らなかった

## 決めたこと

### 配置の表

- `Program` に `layouts: Vec<Layout>` を足し、`LayoutId(u32)` で引く
- `Layout` は、名前 (`name`) と、タグの順に並んだコンストラクタ (`constructors`) を持つ。コンストラクタは、名前と、宣言したフィールドの型の Repr の列 (`fields`) を持つ
  - フィールドの Repr は、コンストラクタの型スキームのフィールドの型から決める。型変数は `tobj` である。具体化は見ないので、`data Pair a = Pair a Int` は `Pair(tobj, int)` になり、`Option Int` と `Option a` は同じ配置 `Option { None, Some(tobj) }` を使う
  - 組の配置は、宣言がないので、すべてのフィールドを `tobj` にする
  - 配置そのものの Repr は持たない。今の型の規則と同じく、コンストラクタのフィールドの数から決める (フィールドのないコンストラクタだけなら `enum`、すべてがフィールドを持てば `obj`、混ざれば `tobj`)。translate の型の Repr と配置の Repr は、同じ関数で決める
  - コンストラクタの名前は、テキストの形と、後の値の表示とデバッグ情報のためにある。何も解決しない
- 配置を持つのは、translate が出す IR が指すデータ型 (Prelude の `Bool` を含む) と、組の大きさごとの配置である。extern の型、`Unit`、レコードは持たない。レコードは S4 で同じ表に、コンストラクタが1つの配置として入る
- 名前は、関数と効果と同じく、入口以外のモジュールの型を修飾する (`Prelude.Bool`)。組の配置の名前は `(,)`、`(,,)` のように、大きさより1少ないカンマを括弧で囲む。組のコンストラクタの名前は配置の名前と同じである
- translate は、配置を最初に使った順に表に入れる。`strings` と同じである。後のパスは表を変えないので、使われなくなった配置が残ることがある

### IR

- `Ctor { layout: LayoutId, tag: u32 }` を足す。タグは配置の中の添字 (宣言の順) である
- `Rhs::Con`、`Stmt::Unpack`、`Stmt::Release` は、タグの代わりに `Ctor` を持つ
- `Term::Switch` は `layout: Option<LayoutId>` を持つ。タグの case を持つ `switch` だけが配置を持つ。`if` と条件の `Bool` の `switch` も含む。リテラルの `switch` と、`default` だけの `switch` は持たない
- `CasePattern::Tag` と `Atom::Tag` は変えない。配置は `switch` に1つだけ置く
- translate は、命令を出すそれぞれの所で、コンストラクタか型から配置を決める。値の型からは決めない。Perceus の `release` は、もとの `unpack` の `Ctor`、または `switch` の配置と case のタグを写す。Perceus は、配置のないタグの `switch` を見たらパニックする (verifier が拒む形なので、黙って飛ばさない)
- translate の `single()` が、`obj` の変数でない値に1つの case の `switch` を出す分岐を消す。フィールドを持つコンストラクタが1つの型の値は、いつも `obj` の変数である。[Core IR とインタプリタ](../../spec/core-ir.md) のその文も消す

### テキストの形

- 先頭に配置の行を表の順に置き、その後に効果の行、関数を置く

  ```
  layout Prelude.Bool { False, True }
  layout Option { None, Some(tobj) }
  layout Pair { Pair(tobj, int) }
  layout (,) { (,)(tobj, tobj) }
  layout Void {}
  ```

- 命令は配置を名前で指す。`switch` は、scrutinee と `{` の間に配置を書く

  ```
  con Option #1(x.1)
  switch o.0 Option { #0 -> b1, #1(n.2: int) -> b2 }
  switch c.1 Prelude.Bool { #0 -> b1, #1 -> b2 }
  unpack p.0 (,) #0(a.1: obj, b.2: obj)
  release xs.0 List #1(x.1, _)
  ```

- 表にない ID は `#N` と書く。`pretty` は表にある ID を名前で、表にない ID を `#N` で書く。`parse` は、`mask` や操作の番号と同じく、表にある ID も `#N` で読む
- `parse` は次を拒む。文言はテストの期待値である
  - 同じ名前の配置が2つある: ``layout `Option` is declared twice``
  - 表にない名前: ``unknown layout `Optoin` ``
  - フィールドの列が空 (`None()`): `an empty field list`
  - `#` で始まる配置の名前: ``a layout name cannot be `#N` ``
  - 組の名前の配置の形が違う (コンストラクタが1つで、その名前が配置と同じで、フィールドの数がカンマの数より1多く、すべて `tobj`、でない): ``the tuple layout `(,)` must have one constructor `(,)` with 2 tobj fields``
  - 配置を書くべき所に名前も `#N` もない: ``expected a layout, found `(` ``
  - 古い形 `con #1(x.1)` は、`#1` を表にない配置として読んだ後、``expected a tag `#N`, found `(` `` で拒む
  - 配置の行が効果の行の後にある: 今の ``expected `fn` `` の誤り
- 往復 (pretty → parse → pretty) は、配置の名前が重ならない Program で同じテキストに戻る

### extern の表

- `Repr` を `eml_extern` に移し、`eml_core_ir` から再公開する。`eml_extern` は依存のないままにする
- 型の行は、使われていない `heap` の代わりに `repr: Repr` を持つ (`Int` は `int`、`String` と `File` は `obj`、`Unit` は `unit`)
- 関数の行は、`arity` の代わりに `params: &'static [Repr]` と `ret: Repr` を持つ。引数の数は `params.len()` である。`by_type` の行 (`==`、`!=`) も、シグネチャどおり `[tobj, tobj]` と `enum` を持つ
- translate の `types.rs` は、extern の型の Repr を行から読む
- extern が作るデータは、今は比べる extern の `Bool` と、`Fs.read_all` の組である。`Bool` のタグは機械の定数で決まっていて (下の結び付けのテストが確かめる)、組の配置は大きさだけで決まるので、行は配置を持たない。S4 で std の extern が名前的なデータ (`List`、`Option`、`Result`) を作るようになったら、行がその型の正規の名前を持ち、translate がその配置を表に入れる。この規則はロードマップの S4 に書く
- 結び付けのテスト (`crates/eml_core_ir/tests/externs.rs`) を足す。translate の型から Repr への規則を、公開の `eml_core_ir::type_repr` で使う
  - 各関数の行の `params` と `ret` を、std のシグネチャの型の Repr と比べる
  - 各型の行の `repr` を、型検査がその型に与える型の Repr と比べる (`Unit` は空のレコード)
  - `by_type` でない関数の行のシグネチャに型変数がないことを確かめる。多相な extern が入る S4 で、行の Repr の比べ方を決め直す
  - Prelude の `Bool` の配置のコンストラクタが `[False, True]` で、機械の `FALSE`/`TRUE` と合うこと、組のタグ `TUPLE` が 0 であることを確かめる

### verifier

検査はどちらの段 (scope の段と所有の段) でも走る。spec では、R8 (Repr) に extern の検査を足し、配置の規則を新しく R9 として書く。今ある形の検査を先に行い、その文言は変えない。新しい検査は、その次で、範囲と所有の検査 (R5、R6) より前に行う。どちらの段でも同じ文言で報告するためである。文言の後には、今と同じく ``in `f` `` が付く。

- R8 (extern): 引数の数を確かめた後で、引数と結果の Repr を行と比べる。定数は今の当てはめの規則 (`Int` は `int`、`()` は `unit`、`#N` は `enum` か `tobj`、`&f` は `tobj`) で比べる
  - ``argument 0 of `Prelude.+` is `s.0` (obj), but the extern takes int``
  - ``argument 1 of `Prelude.+` is (), but the extern takes int``
  - `` `t.1` (obj) is bound to `Prelude.<`, which returns enum ``
- R9 (配置)
  - ID が表にある: ``a con refers to the unknown layout #3`` (`a switch`、`an unpack`、`a release` も同じ形)
  - `switch` が配置を持つのは、タグの case を持つときだけである: `a switch with tag cases has no layout`、``a switch without tag cases has the layout `Prelude.Bool` ``
  - タグが範囲にある: ``a con names #2, but `Prelude.Bool` has 2 constructors`` (`a case`、`an unpack`、`a release` も同じ形)
  - フィールドの数がコンストラクタと同じである
    - ``a con of `Option` #1 has 2 fields, but the constructor has 1``
    - ``a case of `Option` #1 has 0 fields, but the constructor has 1``
    - ``an unpack of `p.0` as `Pair` #0 has 1 fields, but the constructor has 2``
    - ``a release of `d.0` as `Option` #1 has 2 fields, but the constructor has 1``
    - 今の ``an unpack of `p.0` binds no fields`` は、この検査に含まれるので消す
  - タグの `switch` の scrutinee は変数で、その Repr は配置の Repr と同じである
    - `` `c.1` (int) is switched on as `Prelude.Bool`, which is enum ``
    - ``a switch on `Option` has the constant #1 as its scrutinee``
  - `default` のないタグの `switch` は、すべてのコンストラクタを case に持つ: ``a switch on `Option` has no default and no case for #0``
  - リテラルの `switch` の scrutinee の Repr は、`Int` のリテラルなら `int`、`String` のリテラルなら `obj` である
    - `` `s.0` (obj) is switched on Int literals ``
    - `` `n.0` (int) is switched on String literals ``
    - ``() is switched on Int literals``
  - `unpack` は、コンストラクタが1つの `obj` の配置にだけ使える。今の「`obj` の変数だけ」の検査の後で確かめる
    - `` `p.0` (obj) is unpacked as `U`, which is enum ``
    - ``an unpack of `s.0` names `Shape`, which has 2 constructors``
  - `con` の束縛の Repr は配置の Repr と同じである: `` `d.1` (obj) is bound to a con of `Option`, which is tobj ``
  - 宣言した Repr が `tobj` でないフィールドでは、`con` の引数と、case と `unpack` の束縛の Repr が、その Repr と同じである。定数は当てはめの規則で比べる
    - ``field 1 of `Pair` #0 is `n.2` (obj), but the layout has int``
    - ``argument 1 of a con of `Pair` #0 is (), but the layout has int``
  - `release` の出どころは `(値, Ctor, 位置)` である。数は配置で確かめるので、出どころから外す: `` `x.1` is not field 0 of `d.0` as `Option` #1 ``
- `tobj` のフィールドの検査は S3b-2c-2 で入れる。そこでは、`obj` の値は変換なしで `tobj` のフィールドに置けて、`tobj` のフィールドは `obj` の変数に束縛できる。スカラーと参照の間だけ `box` と `unbox` が要る。この関係を [Core IR とインタプリタ](../../spec/core-ir.md) の配置の節に今書く
- 呼び出しの引数と結果、`apply`、`perform`、`resume`、`handle`、`return` と `tail` の比較は S3b-2c-2 で入れる。`the_result_of_a_call_is_not_compared_with_the_callee` はこの段では変えない
- R9 は、それぞれの命令を、その命令が指す配置と比べる。値がどの配置で作られたかは追わない。それを保証するのは translate の型である。そのため、配置の違う値を読む IR が verifier を通ることはありうる

### インタプリタとランタイム

- 機械は配置の表を読まず、`ctor.tag` だけを使う。`Payload::Data` は配置の ID を持たない
- 機械の `unpack` のタグと数の検査、case のフィールドの数の検査、`release_fields` の `WrongLayout` は、内部の誤りの見張りとして残す。上の理由で、verifier を通った IR でも起きうる

## 対象外

- S3b-2c-2 の項目 (上の「段の分け方」)
- 上の「段の分け方」で S3b-2c から外す3つ
- 配置の ID の順を translate のたどり方によらない正規の順にすること。定義ごとに Core IR を保存する REPL の段で入れる。ロードマップの REPL の節に書く
- extern の行の、引数ごとの所有と借用の列。借用パラメータと一緒に入れる。ロードマップの借用パラメータの約束に書く

## テスト

### 足すテスト

- **テキスト** (`eml_core_ir/tests/text.rs`): データ、組の `(,)` と `(,,)`、空の配置、4つの命令それぞれの表にない `#N` を含む往復。上の `parse` の誤りの文言ごとのテスト
- **verifier** (`eml_core_ir/tests/verify.rs`): 上の R8 と R9 の文言ごとのテスト。同じ配置で範囲の中の違うタグを名前に書いた `release` が出どころの文言で拒まれること
- **translate** (`eml_core_ir/tests/translate.rs`): 総称的なデータのフィールドの Repr (`Pair a Int` が `Pair(tobj, int)`)。2つの具体化の `Option` が同じ配置を使うこと。`if` が `Prelude.Bool` を指すこと。組の大きさ2と3の配置。使わない `data` の型が表に入らないこと。ほかのモジュールの型が修飾され、入口のモジュールの型が修飾されないこと。短い名前が同じ2つのモジュールの型が別の配置になること。最初に使った順に番号が振られること
- **extern** (`eml_core_ir/tests/externs.rs`、新しいファイル): 上の結び付けのテスト
- **eml_interp** (`eml_interp/tests/data.rs`): 配置の違う値を読む IR が verifier を通り、実行すると機械の内部の誤りになること (R9 の限界を固定する)

### テストの変更

[テスト戦略](../../implementation/testing.md) の「テストの変更の運用」の種類で書く。どれも試作で確かめた。

**成否の変更**

- なし

**期待値の変更 (範囲)**

- `con`、タグの `switch`、`unpack`、`release` を含むすべての Core IR のテキスト (スナップショットと手書き) で、命令に配置の参照が付き、先頭に配置の行が付く。対象は `eml_core_ir/tests/` の `translate.rs`、`perceus.rs`、`verify.rs`、`text.rs`、`contract.rs` と、`eml_interp/tests/` の `data.rs`、`closures.rs`、`run.rs` である
- verifier の文言
  - `an_unpack_without_fields_is_rejected` は、`unpack` の数の文言になる
  - `a_release_keeps_only_the_fields_of_its_value` は、出どころの文言に ``as `L` `` が付く。組の配置に範囲の外のタグや違う数を書く検査は、範囲と数の文言になる
- `eml_core_ir/tests/contract.rs` の `bindings_used_only_by_removed_bindings_go_in_the_same_pass` の入力は、`con` の束縛の検査に合うように、すべてのコンストラクタがフィールドを持つ配置にする
- `eml_interp/tests/data.rs` の `unpack_of_tag_one` と `release_of_tag_one` のコメントは、検査の見張りが verifier を通った IR でも起きうることを書くように直す。期待する誤りは変えない

**機械的な追随**

- `eml_hir/tests/externs.rs` の `row.arity` を `row.params.len()` にする
- `eml_core_ir/tests/verify.rs` の `the_result_of_a_call_is_not_compared_with_the_callee` のコメントの段の名前を S3b-2c-2 にする。本体と期待値は変えない

## 確認の手順

- `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check`
- 既定でない feature の組み合わせ: `cargo clippy -p eml_cli --all-targets --no-default-features` と `--features types`、`--features core`。`eml_test_support` の同じ組み合わせと `--features hir`
- `cargo test -p eml_cli --test integration citations`
- `nix build`

## 更新する文書

段の終わりに直す。ただし、コードのコメントが引く `docs/spec/core-ir.md` の配置の節の見出しは、そのコメントと同じコミットで足す (citations のテストがあるため)。

- `docs/spec/core-ir.md`: 配置の節 (表、ID、配置の Repr、宣言したフィールドの Repr、配置を持つ型、組、extern が作るデータ、`tobj` のフィールドと `obj` の関係)、文の表と終端の表 (`Ctor` と `switch` の配置)、`unpack` と `release`、R8 の extern、R9、タグは配置の中の添字であること、`single()` の1つの case の `switch` の文を消すこと、verifier の出どころと文言、機械の見張りの理由、レコードが S4 で表に入ること
- `docs/implementation/testing.md`: テキストの形 (配置の行と参照、表にない `#N`)、仕様の例、`switch`、`unpack`、`release` の形、extern の結び付けのテスト
- `docs/implementation/architecture.md`: `types.rs` が extern の行を読むこと、translate の配置の表、`unpack` がコンストラクタが1つの `obj` の配置にだけ使えること
- `docs/spec/runtime.md`: 配置の表が、後のネイティブの物体の記述子のもとになること
- `docs/future/roadmap.md`
  - 段の表の S3b-2c の行と節を「S3b-2c-1 Core IR v2 の表」と「S3b-2c-2 Core IR v2 の境界」に分ける。S4 の前提は S3b-2c-2 にする。S3b-2c-1 の節は段の終わりに削除する
  - `Float` の Repr と `Value::Float` を「`Float`、`Char`、`Num`」の節に移す
  - フィールドのない `#N` の行き先の `decref` を、配置の ID を見てバックエンドが消すことを、ネイティブ化の項目に書く
  - 所有の都合による `TailCall` の降格を、「Perceus の最適化」の借用パラメータの約束に移す
  - S4 に、std の extern が作る名前的なデータの規則と、多相な extern の行の Repr の比べ方を決めることを書く
- `docs/implementation/status.md` の既知の制限 (R9 が値を作った配置を追わないこと、呼び出しの境界と `tobj` のフィールドを S3b-2c-2 まで比べないこと)、`docs/overview.md` の段の一覧、`docs/README.md`、`CLAUDE.md` (`eml_extern` が Repr を持つこと、Core IR の配置の表)、[全体設計](2026-10-07-redesign-design.md) の S3b の記述
- コードのコメント: `eml_extern` のモジュールと行、`Program`、`Stmt::Unpack` と `Stmt::Release`、`Term::Switch`、verifier の冒頭

最後に、`grep -rn -e 'S3b-2c-1' -e 'one-case' docs CLAUDE.md crates` と `grep -rn -e 'arity' -e 'heap' crates/eml_extern` が、意図して残す記述だけを出すことを確かめる。

## 完了の条件

- UI テストの出力が変わらない
- `con`、タグの `switch`、`unpack`、`release` が配置を指し、verifier が R8 の extern と R9 を確かめる
- extern の表が Repr を持ち、translate がそれを読む
- 上の確認の手順がすべて通る
