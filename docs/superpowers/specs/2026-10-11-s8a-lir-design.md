# S8a LIR と移す作業

位置づけ: 作業の設計。ロードマップの S8 の前半を定める。S8a を終えたら、決まったことを `docs/spec/` と `docs/implementation/` に移し、この文書を削除する。

## 目的

Core IR の下に LIR を置く。境界は EP の直後である。box の挿入、縮約、Perceus と所有の verifier を LIR に移し、CEK が LIR を読むようにする。Core IR は、ブロック、Repr、配置、エフェクトの命令を持つ意味の層になり、RC と box と末尾呼び出しを持たなくなる。LIR はエフェクトの命令を型として持たない。S9 の `eml_rt` と S10 の VM は LIR を入力にする。

完了の条件は次の3つである。

- 移す作業の前後で、EP の後、box の挿入の後、縮約の後、Perceus の後のダンプが、すべての対象のプログラムでバイト単位で一致する (下の「バイト単位の一致の確かめ方」)
- CEK が LIR を読んで、全 UI テストを `debug_heap` 付きで通す。`RunStats` を固定したベンチのスナップショットは変わらない
- テストの成否と期待値は、下の「テストの変更」に挙げたものだけが変わる

## S8 の分け方

ロードマップの S8 を、2つの作業に分けて順に進める。ロードマップの段の列では S8 の1行のままにする。

| 作業 | 中身 | テストの変更 |
|---|---|---|
| S8a (この文書) | LIR の型、テキスト形式、パーサ、verifier、Core IR から LIR への変換。box の挿入、縮約、Perceus を LIR に移し、CEK が LIR を読む | 機械的な追随と、表せなくなる形のテストの削除と書き換えだけ。ダンプは変えない |
| S8b | Perceus の所有の形 (フィールドを死ぬ所で手放す形) と `Lin` の move。Kind の印、V1 と V2、extern の行の Kind の列 | 期待値の変更 (ダンプとベンチの回数) |

分けたのは、ロードマップの完了の条件が、バイト単位で一致させる部分と、期待値を変える部分の2つに分かれるためである。所有の形と Kind の印は、「`Lin` の物体の RC は常に 1」という同じ目標を持ち、所有の形の変更を V1 で確かめられるので、S8b にまとめる。

## crate の構成

依存は次の向きにする。`eml_lir` は `eml_extern` だけに依存する葉の crate である。

```
eml_cli ──> eml_interp ──> eml_runtime
   │            │
   │            ├──> eml_lir ──> eml_extern
   │            └──> eml_extern
   ├──> eml_lir
   └──> eml_core_ir ──> eml_lir
```

- `eml_interp` は `eml_lir` だけを読む。後の VM も同じで、フロントエンドなしにビルドできる
- Core IR から LIR への変換は、両方の型を知る `eml_core_ir` に置く
- `eml_cli` と `eml_test_support` の `core` の feature は、`eml_core_ir` と `eml_lir` の両方を入れる。`eml_lir` はフロントエンドに依存しないので、段の feature は増やさない
- `eml_core_ir` は共有する型を再公開しない。使う側は `eml_lir` から読む。`Repr` も `eml_lir` が再公開するものを使う

### `eml_lir` に置くもの

- LIR の型
  - `Program`: 関数、入口、文字列の表、配置の表、エフェクトの表、ファイルの表。今の Core IR の `Program` と同じ欄を持つ
  - `LirFn`: 名前、`internal`、`yields`、`clause` の印、変数の表、`ret`、ブロックの列。印は今の `CoreFn` と同じ意味で、box の挿入 (`internal` と `clause`)、`f$boxed` (`yields` を写す)、S10 の VM が読む
  - `Block`、`Stmt` (`let`、`unpack`、`dup`、`decref`、`release`)、`Term` (`return`、`tail`、`jump`、`switch`)
  - `Rhs`: 呼び出しと `saved`、`closure`、`con`、`const`、`extern`、`prim`、`drop`、`box`、`unbox`。呼び出しは `mask` を持たない
  - `Call`: `call`、`apply` (`yields` の印付き)、`clause` の3つ
- 2つの IR で共有するもの
  - 型: `Atom`、`Case`、`CasePattern`、`FnIdx`、`VarId`、`BlockId`、`VarInfo`、`Layout`、`LayoutCtor`、`LayoutId`、`Ctor`、`Loc`、`Prim`、`PrimRow`、`EffectInfo`、`OperationInfo`、`OpRet`
  - 定数: `FALSE`、`TRUE`、`LT`、`EQ`、`GT`、`TUPLE`
  - 関数: `data_repr` (translate の型の Repr と配置の Repr を同じ関数で決めるため、translate も使う)、`operation` (エフェクトの表の操作を引く。EP も使う)、`Prim` の `by_name` と `immediates` と `Immediates`
  - `Atom::fits` と `Atom::fits_compatible` は、今は `&CoreFn` を受ける。共有する型の固有のメソッドは別の crate で定義できないので、変数の表 (`&[VarInfo]`) を受ける形に改めて公開する
- テキスト形式 (`pretty`、`pretty_with_positions`、`parse`)
- verifier の3つの段 (下の「verifier の段」)
- 生存解析 (`liveness`、公開)、末尾の位置の呼び出しの判定と純粋な右辺の判定 (下の「末尾の位置の判定の写し」)、box の挿入 (`boxing`)、縮約 (`contract`)、Perceus (`perceus`) と、その順番 (下の「パスの順番」)。3つのパスは、テストが直接呼べるように、それぞれ公開する
- Core IR が借りる部品を、公開のモジュールで出す
  - テキスト形式: 字句解析、アトム、Repr、配置、エフェクトの宣言、関数の名前の読み書きと引用符。今の `pretty.rs` の `effect_text`、`effects_text`、`operation_text`、`prim_text`、`function_name` は、`&Program` でなくエフェクトの表 (`&[EffectInfo]`) や名前を受ける形にする。EP も `mask$[…]` の名前に `effects_text` を使う
  - verifier: 支配木 (`Dominators`、`GrowingTree`)、R8 と R9 の検査、基本操作と `clause` の検査 (今の「EP の段の検査」)
  - 部品は共有する型の上で書き、IR ごとに違うもの (変数の Repr、ブロックの行き先の並び、文ごとの定義と使い) は引数で受ける。命令の列をたどる部分は IR ごとに書く

### `eml_core_ir` に残すもの

- Core IR の型: `Program`、`CoreFn`、`Block`、`Stmt` (`let`、`unpack`)、`Term` (`return`、`jump`、`switch`)、`Rhs` (呼び出しと `mask`、`closure`、`con`、`const`、`extern`、`prim`、`drop`)、`Call` (`call`、`apply`、`clause`、`handle`、`perform`、`resume`)。`dup`、`decref`、`release`、`box`、`unbox`、`tail`、`saved` は型から消える
- translate、EP、`type_repr`
- EP が使う生存解析と、末尾の位置の呼び出しの判定と純粋な右辺の判定。どれも Core IR の型の上の写しで、crate の外には出さない
- Core IR のテキスト形式と、verifier の変換の段と EP の段
- Core IR から LIR への変換 `to_lir`
- extern の行と `std/` のシグネチャを結ぶテスト (`tests/externs.rs`)

### 末尾の位置の判定の写し

- EP は今、`boxing.rs` の `tail_position_call` と、それが使う `contract.rs` の `pure` を借りている。末尾の位置の呼び出しを yield の地点にしないためである ([Core IR とインタプリタ](../../spec/core-ir.md) の「yield の地点」)
- box の挿入と縮約が `eml_lir` に移るので、2つの判定を、Core IR の型の上の写しと LIR の型の上の写しの2つにする。どちらも、[Core IR とインタプリタ](../../spec/core-ir.md) の「変換の規則」の「末尾の位置の呼び出し」の1つの定義を実装する
- EP が「末尾の位置の呼び出しは yield の地点にしない」と決めたことと、T3 と縮約が末尾呼び出しを作ることは、同じ定義の上で成り立つ。純粋な右辺の集合は、LIR では `box` と `unbox` を含み、Core IR では含まない。EP の出力には `box` と `unbox` がないので、EP の出力の上で2つの判定は同じ答えを返す

### 型を分けた理由

- 型を読むだけで、どちらの IR か分かる。LIR にエフェクトの命令がないことと、Core IR に RC と box がないことを、verifier でなく型で保証できる
- 重複は、命令ごとの表示、読み込み、検査、命令の列をたどる部分のうち、2つの IR に共通する定型の部分である。重なる部品は `eml_lir` から借りる
- Lean (LCNF と IR) と Koka も、層ごとに別の型を持つ

## パスの順番

各 crate は、自分の層のパスの順番だけを知る。

- `eml_core_ir::Pass` は `Translate`、`Evidence` の2つである。`eml_core_ir::core_until(hir, typed, entry, files, Pass) -> eml_core_ir::Program` は、指定したパスの直後で止める
- `eml_lir::Pass` は `Boxing`、`Contract`、`Perceus` の3つである。`eml_lir::passes_until(&mut Program, Pass)` は、変換の直後の LIR に、box の挿入から指定したパスまでを順にかける
- 全体の入口は `eml_core_ir::lower(hir, typed, entry, files) -> eml_lir::Program` と `eml_core_ir::lower_until(.., eml_lir::Pass) -> eml_lir::Program` である。translate、EP、`to_lir`、`passes_until` の順に呼ぶ。2つの層をつなぐ順番を知るのは、この2つの関数だけである
- verifier はデバッグビルドだけでかけ、誤りはパスの名前を付けて報告する
  - Core IR のパスの後: 今と同じ `internal error: invalid Core IR after {pass}: {error}`
  - `passes_until` は、最初に入力に `verify_lowered` をかける。誤りは `internal error: invalid LIR before boxing: {error}` と報告する。box の挿入の入力の約束であり、手で書いた LIR を `passes_until` に渡すテストもこの検査を通る
  - LIR のパスの後: `internal error: invalid LIR after {pass}: {error}`
  - `to_lir` の直後には別の検査を置かない。`to_lir` の出力は、そのまま `passes_until` の入口の検査を受ける

## Core IR から LIR への変換

- `to_lir(eml_core_ir::Program) -> eml_lir::Program` は、関数、ブロック、変数の表、配置の表、文字列の表、エフェクトの表、ファイルの表を、番号と名前を変えずに写す。入口も同じ関数を指す
- 写すだけで、命令を足しも消しもしない。`handle`、`perform`、`resume` か空でない `mask` が残っていれば、内部の誤りとしてパニックする。EP の段の verifier が先に拒む形なので、捨てて先へ進むことはしない
- 作業の手順 3 と 4 の間は、Core IR がまだ `tail`、`box`、`unbox`、RC の命令、`saved` を持つ。その間の `to_lir` は、これらを LIR の同じ命令に1対1で写す。手順 5 で Core IR から消えると、写す分岐もなくなる
- 配置の表は、変換で LIR に移る。S8a の LIR のパスは配置を足さない
- 変数の名前と番号が変わらないので、「EP の後の Core IR の表示」と「それを `to_lir` した LIR の表示」は同じ文字列になる。下の「残すテスト」で、この性質を確かめる

## テキスト形式

- LIR のテキスト形式は、今の Perceus の後の Core IR の表示と同じ文法である。`saved` は今と同じく `save [...]` で書く
- Core IR のテキスト形式は、今の文法から `box`、`unbox`、`dup`、`decref`、`release`、`tail`、`save [...]` を除いたものである
- 2つの読み込みは、相手の IR にしかない命令の語を、今の未知の語と同じ形の誤りで拒む。LIR の読み込みでは、`tail` の後の `mask` は、今と同じく ``expected a call, found `mask` `` になる
- 終端のないブロックの誤りの文言は、LIR では今と同じ `expected a statement; a block ends with return, tail, jump or switch` で、Core IR では `expected a statement; a block ends with return, jump or switch` にする
- 2つの IR に共通する命令は、同じ表示になる。字句解析と、アトム、Repr、配置、エフェクトの宣言、関数の名前の読み書きは `eml_lir` の部品を使う

## verifier の段

- Core IR の verifier は2つの段を持つ
  - 変換の段 (`verify_translated`): 今の変換の段から、型にない命令 (`tail`、`box`、`unbox`、RC、`saved`) の検査を除いたもの
  - EP の段 (`verify_evidence`): 今の EP の段から同じものを除いたもの。`handle`、`perform`、`resume`、`mask` を今と同じ文言で拒む
- LIR の verifier は3つの段を持つ
  - 入口の段 (`verify_lowered`、新設): `passes_until` が入力にかける。今の EP の段から、エフェクトの命令の拒否を除いたものである。`tail`、`box`、`unbox`、`dup`、`decref`、`release`、空でない `saved` を、今と同じ文言 (``a tail call is formed before contract``、`` `n.1` is boxed before the boxing pass ``、`` `s.0` is duplicated before Perceus `` など) で拒む
  - 範囲の段 (`verify_scopes`): box の挿入と縮約の後にかける。今と同じ
  - 所有の段 (`verify`): Perceus の後にかける。今と同じ
- 基本操作と `clause` の検査 (今の [Core IR とインタプリタ](../../spec/core-ir.md) の「verifier」の「EP の段の検査」) は、Core IR の EP の段と、LIR の3つの段のすべてで走る
- 末尾呼び出しの文言のうち、`a tail perform`、`a tail resume`、`a tail handler` は、どちらの IR でも書けない形なので消す
- ロードマップの「層の分担」の表は、verifier の段を「Core IR (translate の後、EP の後) と LIR (スコープ、所有)」としていた。LIR に入口の段を足すので、この行を改める

## CEK

- `eml_interp::run` と `eml_cli::execute` は `&eml_lir::Program` を受ける。機械の動作は変えない
- 内部の誤り `an effect instruction reaches the interpreter` は、型で表せなくなるので消す。この誤りを確かめるテストはない
- `saved` は、S10 で CEK を消すまで LIR に残す。CEK だけが使う (ロードマップの論点「`saved` を S10 まで LIR に残す形」の決定)

## 呼び出す側

- `eml_cli::Session`
  - 結果の型を `Compiled<P>` にし、`program: Option<P>` を持たせる
  - `compile` は `Compiled<eml_lir::Program>` を返す。`compile_until(eml_lir::Pass)` も同じ型を返す
  - Core IR の段で止める `compile_core_until(eml_core_ir::Pass) -> Compiled<eml_core_ir::Program>` を足す
- `eml_test_support`
  - `core_until(text, eml_core_ir::Pass)` と `core_until_files` は Core IR を返す
  - 最終の IR は `lir(text)` と `lir_files`、途中の LIR は `lir_until(text, eml_lir::Pass)` と `lir_until_files` で得る。今の `core(text)` と `core_files` は `lir` と `lir_files` に改名する
  - `execute` と `run_stats` は `eml_lir::Program` を扱う
- `eml_core_ir` の `tests/common` の補助 (`core_text` など) と、`eml_interp` の `tests/common` の補助も、同じ分け方にする。`eml_interp` の `run_core` と `run_core_unverified` は `run_lir` と `run_lir_unverified` に、`tests/ir/*.core` は `tests/ir/*.lir` に改名する

## ロードマップの論点のうち、S8b に送るもの

- 基本操作の引数を借用にするか
- 借りたまま読む射影の命令
- yield の地点の結果のダミーへの `decref` と V1 の関係
- V2 で `poly` から `lin` へ渡す箇所が、正しいプログラムに出ないか (下の「S8b への申し送り」)
- LIR が配置を足せるか。S8a の LIR のパスは配置を足す必要がないので、S8a では決めない
- S8 の中の順序のうち、所有の形を変える作業と Kind の印の順序

### S8b への申し送り

- ロードマップの V2 の記述は向きが食い違っている。本文は「順序は `unr ≤ poly ≤ lin` で、受け手の印が送り手の印以上」とし、論点は「`lin` から `poly` への受け渡しは許され、逆は拒まれる」とする。前者では `lin` から `poly` が拒まれる
- さらに、一様な位置の関数 (`depth@[_]` が型変数の値を返す形) を、その型変数が `File` か `Int` の instance の呼び出し元が受けると、`poly` から `lin`、`poly` から `unr` の受け渡しが、正しいプログラムに出る。呼び出し元の型は具体的で正しいためである
- そのため S8b では、Repr の互換と同じ形の関係 (`poly` は `unr` とも `lin` とも互換で、`unr` と `lin` は互換でない) を第一の案として検討する

## バイト単位の一致の確かめ方

### 移す作業の前後 (一時的なテスト)

- S8a の最初の作業で、一時的なテスト `crates/eml_core_ir/tests/s8a_baseline.rs` を足す (`tests/main.rs` で宣言する)。`#[ignore]` を付け、普通の `cargo test` では走らせない。基準のダンプは手元にしかなく、新しい worktree や `cargo clean` の後には存在しないためである
- 対象は、ワークスペースの根の `tests/ui/run` と `tests/ui/run-fail` のすべてのテスト (1ファイルのものと、ディレクトリの `main.em` と同じディレクトリのモジュールのもの) と、`bench/` の6本のプログラムである
- それぞれについて、EP の後、box の挿入の後、縮約の後、Perceus の後の4つのダンプを、`pretty_with_positions` で作る
- 基準のダンプの場所は、環境変数 `EML_S8A_BASELINE` (絶対パスのディレクトリ) で渡す。渡されていなければ、テストは理由を書いて失敗する
  - `EML_S8A_BLESS=1` も付けると、ダンプをそのディレクトリに書く
  - 付けなければ、書いてあるダンプと比べ、違えば差分を出して失敗する。基準のダンプがなければ失敗する
- 基準のダンプは、基準の版 (`fa1a01e`) の上にこのテストだけを足した版で書く。コミットはしない。数 MB になりうるうえ、基準の版からいつでも作り直せるためである
- 各手順の確かめでは、全テストに加えて `EML_S8A_BASELINE=<dir> cargo test -p eml_core_ir --test integration s8a_baseline -- --ignored` を走らせる
- 手順 4 で API が変わるので、このテストを書き換える。EP の後のダンプは `core_until(.., eml_core_ir::Pass::Evidence)` から、ほかの3つは `lir_until` から作る
- S8a の最後の作業で、このテストを消す。S7 の差分テストと同じ扱いである

### 残すテスト

- `crates/eml_core_ir/tests/lowering.rs` (新設): 上と同じ UI テストのプログラムすべてで、「EP の後の Core IR の `pretty_with_positions`」と「それを `to_lir` した LIR の `pretty_with_positions`」が同じ文字列であることを確かめる。変換が構造を写すだけであることと、2つの表示が離れていかないことを見張る
- 実行の結果は、UI テストと、`crates/eml_interp/tests/bench.rs` の `RunStats` のスナップショットが今のまま通ることで確かめる

## テストの変更

[テスト戦略](../../implementation/testing.md) の「テストの変更の運用」の種類で書く。

### テストの置き場所

- `crates/eml_core_ir/tests` の `boxing.rs`、`contract.rs`、`perceus.rs` は、手で書いた IR のテストとソースからパイプラインを通すテストが混ざっている。それぞれを2つに分ける
  - 手で書いた IR のテスト (`boxing_text`、`contract_text`、`perceus_text` を使うものと、生存解析のテスト) は、`crates/eml_lir/tests` の同じ名前のファイルに移す。補助 (`function` など) は `eml_lir` のテストの側に写す。テストは `boxing`、`contract`、`perceus` を直接呼ぶ。`eml_lir` の dev-dependency にフロントエンドを入れないためである
  - ソースからパイプラインを通すテスト (`core_text` を使うもの) は、`crates/eml_core_ir/tests` に残す。パイプラインをつなぐのが `eml_core_ir` だからである
- `verify.rs` と `text.rs` は、テストごとに次の規則で置き場所を決める
  - LIR だけの命令 (`tail`、`box`、`unbox`、RC、`save`) か、LIR の段 (範囲の段、所有の段) を確かめるテストは `eml_lir` に移す
  - Core IR だけの命令 (`handle`、`perform`、`resume`、`mask`) か、Core IR の段 (変換の段、EP の段) を確かめるテストは `eml_core_ir` に残す
  - 2つの IR に共通する文法だけのテキスト形式のテストは `eml_lir` に移す。字句解析と共通の命令の読み書きは `eml_lir` の部品なので、そこで確かめる
  - 両方を含むテストは、下の「機械的な追随」と「期待値の変更」と「成否の変更」に挙げたとおりに分けるか書き換える
- `prim.rs` の単体テスト `every_kind_has_its_own_name` は、`Prim` と一緒に `eml_lir` に移す。そのため、`eml_lir` の lib は `test = false` にしない
- `eml_interp` のテスト (単体テストの `src/prims.rs` を含む) は、`eml_lir` の型と `eml_lir::parse`、`lir(..)` を使う

### 機械的な追随

合意は要らない。記録はコミットメッセージで足りる。スナップショットの文字列と、`assert` で比べる文字列と値は変えない。

- 上の置き場所へのテストの移動。テストの名前は、段や IR に合わせて改めてよい
- `core` から `lir` への改名、`run_core` から `run_lir` への改名、`tests/ir/*.core` から `*.lir` への改名、共有する型を `eml_lir` から読むための `use` の変更
- パイプラインの止め方の変更: `eml_core_ir::lower_until(.., Pass::Translate)` を `core_until` に (`instances.rs` の2か所、`translate.rs` の1か所)、`eml_test_support::core_until(.., Pass::Perceus)` を `lir_until` か `lir` に (`translate.rs` の2か所と `tests/common/mod.rs`) 替える
- 1つのテストが Core IR の段と LIR の段の両方を確かめるときは、IR ごとに2つのテストに分ける。どの `assert` も、どちらかの crate に残る。当たるのは、`verify.rs` の `a_jump_passes_values_to_compatible_parameters`、`a_returned_value_is_compatible_with_the_function`、`a_returned_value_that_is_not_compatible_is_rejected`、`the_translated_level_allows_a_function_value_that_is_not_uniform`、`prims_and_clauses_are_accepted_after_the_evidence_pass`、`a_clause_function_has_the_reprs_of_its_operation`、`a_clause_function_is_only_put_into_a_handler_table` などである。`eml_cli/tests/api.rs` の `compile_until_stops_after_the_named_pass` と `eml_test_support/tests/support.rs` の `core_until_stops_after_the_named_pass` も、Core IR の段と LIR の段を1つの `pretty` で比べているので分ける
- Core IR の変換の段と EP の段で、`tail call`、`tail apply`、`box`、`unbox`、`dup`、`release`、`save` を拒むテストは、LIR の `verify_lowered` で同じ文言を確かめるテストに移す。`verify.rs` の `the_translated_level_rejects_tail_calls` (`tail call` と `tail apply` の部分)、`the_translated_level_rejects_boxes_and_unboxes`、`the_translated_level_rejects_what_perceus_inserts`、`the_evidence_level_still_comes_before_boxing_and_contract` が当たる。`boxing.rs` の `boxing_text` が入力を `verify_translated` か `verify_evidence` で確かめる所と、`t3_takes_time_linear_in_the_chain` が `verify_translated` を呼ぶ所も、`verify_lowered` にする
- `verify.rs` の `a_mask_on_a_clause_is_rejected` で、`verify_scopes` に `a mask remains after the evidence pass` を確かめる `assert` は、`verify_evidence` で同じ文言を確かめる形にする
- 誤りを確かめる手書きの IR の入力のうち、どちらの IR でも書けない形を、期待する文言と行を変えずに書ける形に直すもの
  - `text.rs` の `malformed_declarations_names_and_strings_are_errors_with_their_lines` は表で書いた誤りのテストで、共通の文法の行と Core IR の行が混ざっている。`tail perform B.b(1)` と `tail handle A(..)` の2つの入力を `let` と `return` の形に直して Core IR に残し、ほかの行は `eml_lir` に移す。誤りの行は変わらない
  - `text.rs` の `an_empty_mask_or_save_is_an_error` は、`mask` の部分を Core IR に、`save` の部分を LIR に分ける。`a_tail_call_takes_no_mask` は LIR に移す
- IR の型の欄が減ったことに合わせて、`assert` の中の構造体の書き方から、型にない欄を除くもの。Core IR の `Rhs::Call` から `saved` を、LIR の `Rhs::Call` から `mask` を除く (`text.rs` の 684、724、754、796、1606 行のあたり)。比べる値は変わらない

### 期待値の変更

往復を確かめるテキスト形式のテストでは、入力の IR がそのまま期待する表示である。どちらの IR でも書けない形 (末尾の位置のエフェクトの命令と、`mask` と `save` を両方持つ呼び出し) を入力にしている次のテストは、入力を書ける形に直すので、期待する表示が変わる。

| テスト | 変更 |
|---|---|
| `eml_core_ir` `text.rs` `a_handle_round_trips` | `tail handle Done((), b.1) {} return r.4` を `let` と `return` の形にする |
| 同 `a_perform_round_trips` | `tail perform never Ask.stop(t.1)` と `tail perform never Ask.ask(1)` を `let` と `return` の形にする。`Term::TailCall(Call::Perform { .. })` を確かめる `assert` は、`Stmt::Let` の `Call::Perform` を確かめる形にする |
| 同 `a_resume_round_trips` | `tail resume ()(t.2, ())` を `let` と `return` の形にする |
| 同 `a_mask_reads_back` | `mask [#3] call f(c.0, k.1) save [c.0]` から `save [c.0]` を除いて Core IR に残す |
| 同 `successors_and_atoms_come_in_a_fixed_order` | `handle` と `tail apply` を1つの関数に持つ。`handle` の部分を Core IR のテストに、`tail apply` の部分を LIR のテストに分け、それぞれの入力を組み直す。`for_each_atom` と行き先の順を確かめる値は変えない |

ほかのダンプ、診断、UI テストの出力、`RunStats` は変えない。ロードマップは「Perceus の後の Core IR のダンプは、LIR のダンプになる」ことを期待値の変更として挙げていたが、LIR のテキスト形式を今の表示と同じ文法にしたので、文字列は変わらず、テストの移動は機械的な追随で済む。作業の中で範囲の外の変更が要ると分かったら、この spec に戻して書き足してから進める。

### 成否の変更

どちらの IR でも表せなくなる形を確かめる `assert` を消す。

| テスト | 変更 |
|---|---|
| `eml_core_ir` `verify.rs` `the_translated_level_rejects_tail_calls` | `tail perform`、`tail resume`、`tail handle` を拒む3つの `assert` を消す。Core IR には `tail` がなく、LIR にはエフェクトの命令がないので、どちらの IR でも書けない |
| 同 `the_scope_level_rejects_effect_instructions` | テストを消す。範囲の段と所有の段は LIR の段になり、LIR の読み込みがエフェクトの命令を読まないので、入力を書けない。Core IR の EP の段で同じ文言を確かめるテスト (`the_evidence_level_rejects_effect_instructions`) は残る |

### 新しいテスト

- `crates/eml_core_ir/tests/lowering.rs`: 上の「残すテスト」。手で書いた小さな Core IR でも、`to_lir` が名前、番号、印、配置、エフェクトの表を変えないことを確かめる
- `crates/eml_lir/tests/verify.rs`: `verify_lowered` が `tail`、`box`、`unbox`、RC、`saved` を拒み (上で移すもの)、基本操作と `clause` の検査を行うこと。`passes_until` が入口で `verify_lowered` をかけること
- `crates/eml_core_ir/tests/text.rs`: Core IR のすべての命令 (`prim`、`clause`、`handle`、`perform`、`resume`、`mask`、`yields` と `clause` の印を含む) を持つプログラムの往復。今の `every_prim_round_trips` は `tail clause` で終わるので LIR に移り、Core IR の基本操作の表示を確かめるテストがなくなるためである。Core IR の終端のないブロックの誤りの文言
- `crates/eml_lir/tests/text.rs` と `crates/eml_core_ir/tests/text.rs`: 相手の IR にしかない命令の語を拒むこと (LIR では `perform`、`handle`、`resume`、`mask`、Core IR では `box`、`dup`、`tail`、`save` など)

## 作業の順序

各手順の終わりに、全テストと一時的なテスト `s8a_baseline` (手順 2 から) が通る。

1. 一時的なテスト `s8a_baseline.rs` を足し、基準のダンプを書く
2. `eml_lir` の crate を作り、共有するもの (型、定数、関数) を `eml_core_ir` から移す。`Atom::fits` と `fits_compatible` を変数の表を受ける形に、テキスト形式の補助をエフェクトの表を受ける形にする。`eml_core_ir` と `eml_interp` は移したものを使う
3. `eml_lir` に LIR の型、テキスト形式、verifier の3つの段、生存解析を作り、`eml_core_ir` に `to_lir` を足す。Core IR はまだ今の命令をすべて持ち、`to_lir` はそれを1対1で写す。LIR のテキスト形式と verifier のテストを `eml_lir/tests` に写して足す。`lowering.rs` を足す
4. 末尾の位置の判定と純粋な右辺の判定の Core IR の写しを作り、EP をそれに切り替える。box の挿入、縮約、Perceus を `eml_lir` に移し、パスの順番を上の形にする。`eml_cli`、`eml_test_support`、`eml_interp` を LIR に切り替える。移すテストを動かし、`s8a_baseline` を新しい API に書き換える
5. Core IR の型、テキスト形式、verifier から、RC、`box`、`tail`、`saved` を消す。Core IR の verifier とテキスト形式を `eml_lir` の部品の上に組み直し、重複を除く。上の期待値の変更と成否の変更を行う
6. 文書を移し、一時的なテストを消す

手順 2〜5 で書くコードのコメントは、今の `docs/spec/core-ir.md` の節を引く。`citations.rs` が、なくなった節への参照を誤りにするためである。手順 6 で、新しい文書の節に引き直す。

## 文書の更新

S8a の終わりに1回で行う。

### `spec/core-ir.md` を分ける

2つの IR に共通する規則と、LIR の命令の意味は `spec/lir.md` に1回だけ書き、`spec/core-ir.md` から引く。LIR は S9 以降のバックエンドの入力なので、共有する規則の置き場所にする。型から決める規則と translate の規則は、型を持つ Core IR の側に置く。

- `spec/lir.md` (LIR、新設)
  - LIR の命令
  - ブロックの列の構造の規則 (R1〜R9)。各規則のうち LIR だけに当たる部分 (R6 の所有の半分、R7、R8 の `tail`、`box`、`unbox` と境界の検査) に印を付ける
  - 値の表現のうち、Repr、互換の関係、正確な位置と互換な位置、定数の当てはめ、`box` と `unbox` の規則、バックエンドが守ること
  - データの配置のうち、表の形、配置の Repr、R9 との関係、バックエンドが表から読むもの
  - 位置の規則 (局所の変数の行を除く)、一様な関数、エフェクトの位置 (操作の表の `params` と `ret`、`OpRet`)、`never` の操作、T3
  - 末尾の位置の呼び出しの定義と、末尾呼び出しの保証。EP も同じ定義を使う
  - 基本操作の表、実行の文脈、evidence と `evv` の物体の意味、ダミーの値の約束、`evv` の不変条件。LIR の `prim` の意味で、CEK、S9、S10 が実装する
  - box の挿入、縮約、生存解析、Perceus
  - LIR の verifier の3つの段
  - インタプリタ (CEK 機械)、実行時エラー、実行時の規約
- `spec/core-ir.md` (Core IR)
  - 実行系の方針 (2つの層の分担)
  - Core IR の命令と、LIR との違い。Core IR が従う構造の規則 (R1〜R5、R6 の支配の半分、R8 と R9 のうち境界の検査でない部分と引き継ぐ検査) を `spec/lir.md` から引く
  - 型から Repr を決める表 (`type_repr`)、translate が配置を決める規則と、`Bool` と `Ordering` のタグを確かめるテスト、位置の規則の局所の変数の行と、translate が決める関数のシグネチャ
  - 変換の規則、row のビットと instance の鍵、`yields` の印、`apply` の印と分け方、メソッドの解決、導出とタプルの生成器、一様な位置と制約付きの多相再帰、位置
  - パス (Core IR のパスと、後のパスの置き場所の規則)
  - evidence passing の変換。基本操作と実行時のモデルは `spec/lir.md` を引く
  - Core IR から LIR への変換
  - Core IR の verifier の2つの段

### ほかの文書

- 移した節を引く文書とコードのコメントの参照先を直す。`citations.rs` の検査が、なくなった節への参照を見つける。引いている文書は `spec/runtime.md`、`spec/effects.md`、`overview.md`、`implementation/` の各文書、`future/` の各文書などである
- `README.md`: 文書の表に `spec/lir.md` を足し、`spec/core-ir.md` の説明を改める。crate と文書の対応の表に `eml_lir` を足す
- `overview.md`: 処理系の説明と用語の表に LIR を足す
- `spec/runtime.md`: CEK が読むのが LIR になること (`dup`、`decref`、`release` は LIR の命令になる)
- `implementation/architecture.md`: crate の表と依存の図、パイプラインとパスの順番、`eml_lir` の内部
- `implementation/testing.md`: テキスト形式の節を Core IR と LIR の2つに分ける。テストの置き場所の表と、手で書いた IR のテストの説明 (今の 51 行目のあたり)
- `implementation/status.md`: 今の段の位置
- `future/roadmap.md`: S8 の節に S8a と S8b の分け方を書く。S8a で決めたこと (LIR の型、テキスト形式、パーサ、verifier、変換、パスを移すこと、CEK が LIR を読むこと、関数などの表を LIR に置くこと、`spec/core-ir.md` を分けること、`saved` の論点) を消す。層の分担の表は、verifier の段の行を上の「verifier の段」のとおりに改めて、S8b の Kind の印の行とともに残すか、決まった行を `spec/` に移す。所有の形、Kind の印、上の「S8b に送るもの」と「S8b への申し送り」は残す
- `CLAUDE.md`: Architecture の crate の図と説明、Core IR のパスの列と verifier の段、`eml_lir` の説明。Testing の `eml_test_support` の関数 (`lir`、`lir_until`)
- ベンチは、S8a ではスナップショットの回数が変わらないことを確かめるだけにする。`bench/run.sh` の命令の数は、S8 の終わり (S8b) に `implementation/benchmarks.md` に記録する

## 採らなかった案

- **骨格を generic にする形** (`Function<D: Dialect>` で、表示、読み込み、verifier を1つずつ持つ): 重複はほぼ消える。しかし、ジェネリクスとフックがすべてのパスと verifier に広がり、verifier の中も所有の検査の有無で分岐する。重複は定型の部分に限られるので、型を分けて部品を共有する形にした
- **Core IR を LIR の型の上の拡張にする形** (`eml_core_ir::Rhs` を、`lir::Rhs` にエフェクトの命令を足した列挙にする): LIR にエフェクトの命令がないことは型で保証できる。しかし、Core IR から RC と box を消せず、ロードマップの「Core IR から RC と box を消す」と合わない
- **`eml_lir` が `eml_core_ir` に依存する形**: 共有する型を動かさずに済む。しかし、VM が `eml_core_ir` を通して `eml_hir` と `eml_types` にも依存し、バックエンドの入力がフロントエンドから独立しない
- **`eml_core_ir` の中のモジュールにする形**: crate は増えない。しかし、層の分離が crate の境界でなく決まりごとに頼る
- **末尾の位置の判定を1つにして、文ごとの純粋さを引数で受ける形**: 定義の写しが1つで済む。しかし、判定は1つのブロックの文と終端を見る十数行で、引数で渡す抽象のほうが大きい。2つの写しが同じ定義を実装することは、上の「末尾の位置の判定の写し」に書いた
- **`to_lir` の直後に別の検査の段を置く形**: 変換の誤りをその場で見つけられる。しかし、`to_lir` の出力は EP の段の検査を通った IR を写しただけで、`passes_until` の入口の検査がすぐに同じことを確かめる
- **基準のダンプをコミットする形**: 別の場所でも比べられる。しかし、数 MB になりうるうえ、基準の版からいつでも作り直せる
