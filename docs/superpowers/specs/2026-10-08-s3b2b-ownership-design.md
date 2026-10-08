# S3b-2b Core IR v2 の所有 (設計)

位置づけ: 作業の設計。再設計のサブプロジェクト S3b-2b の spec である。全体の方針は [ロードマップ](../../future/roadmap.md) の「S3b-2b Core IR v2 の所有と表現」と、[全体設計](2026-10-07-redesign-design.md) にある。決まったことは S3b-2b の終わりに `docs/` の該当文書へ移し、この文書を削除する。

## 目的

`switch` と `unpack` を、scrutinee を消費しない形にする。フィールドは scrutinee から借りた値になり、Perceus がフィールドの `dup` と scrutinee の手放し方を明示する。一意な箱では、新しい文 `release` が箱だけを解放し、使うフィールドは箱の参照を引き継ぐ。

この形にするのは、後で借用パラメータと reuse を、IR を作り直さずに足せるようにするためである。バイトコード VM では、`switch` はタグを読んで分岐するだけの命令になり、`release` は1つの命令になる。ネイティブ化では、`release` をその場で「一意かどうか」の分岐に展開できる。

## 段の分け方

ロードマップの S3b-2b を2つの段に分ける。

- S3b-2b (この文書): 所有。消費しない `switch` と `unpack`、借りたフィールド、`release`、verifier の借用の規則、Perceus の規則、RC の操作の数え方
- S3b-2c: 表現。Repr の確定 (多相な位置の規則、`Float`)、データの配置の表と配置の ID、box/unbox とそれを入れるパス、extern の表の Repr、呼び出しの結果と `ret` の比較、`TailCall` の降格

分ける理由は、2つの変更がコードの上でほぼ独立しているためである。所有の変更は、呼び出しをまたいで所有を動かさない (試作で、すべての `save` の並びが変わらなかった)。表現の変更は、`unbox` を「読む」使い方にするので、この段の使い方の区別の上に載る。

## 背景

- `switch` は scrutinee を消費する。フィールドを束縛する case に入ると、インタプリタは `take_or_copy` で箱を取り出す。一意なら箱を壊してフィールドを取り、共有なら箱を複製してフィールドを `dup` する
- 行き先の1つでも scrutinee を使うと、Perceus は `switch` の前で `dup` する。これで参照の数が2以上になり、`take_or_copy` は一意な値でもいつも複製の側を通る (ロードマップの `| ys -> f ys` の項目)
- 試作では、何も足さずに消費しない形にすると、UI のプログラム全体で RC の操作が 3.35 倍、リストの再帰で 4 倍になった。実行されたフィールドを束縛する `switch` の 99.985% は一意な箱に対するものだった
- Koka (Perceus) と Lean 4 は、どちらも消費しない `match` に、一意な箱の場合をまとめて扱う手段 (Koka の drop specialization、Lean の reset/reuse) を組み合わせている

この spec の規則をすべて実装した試作では、次のことを確かめた。

- UI の run と run-fail の 114 本 (下の「足すテスト」の4本を含む) が、Perceus の後の verifier と `debug_heap` を有効にして、今と同じ出力になった。すべての関数の `save` の並びと、`dup`、`decref`、`release` 以外の IR は、今と同じだった
- RC の操作は、UI の run の 106 本の合計で 127,950 回から 125,942 回 (-1.6%) になった (下の数え方の約束による)。一意なリストをたどる `dup` は長さによらず0回、共有されたリストでは長さと同じ回数で、今と変わらない
- ワークスペースのテストは 23 件が変わり、「テストの変更」の一覧と一致した

## 決めたこと

### 意味と IR

- `switch a { .. }` と `unpack x #t(..)` は、タグとフィールドを読むだけである。参照の数を変えず、scrutinee を消費しない
- scrutinee は、どの行き先でも、`unpack` の後でも、所有されたままである。フィールドのある case、フィールドのない `#N` の case、`default`、文字列の `switch` のどれも同じである。`int` と `enum` のリテラルの `switch` は、何も所有していない
- Repr が `obj` か `tobj` のフィールドは、束縛したときには借りた値である。自分の参照を持たず、上の持ち主が参照を持っている間だけ使える (下の「verifier」)。ほかの Repr のフィールドは、ただの値である
- 新しい文 `release x #t(p1, .., pn)` を足す。`unpack` と同じく、フィールドの位置ごとに、参照を引き継ぐ変数か `_` を書く
  - x は、タグが t でフィールドが n 個のデータでなければならない。違えば内部の誤りである
  - x が一意 (参照の数が1で、不死でない) なら、x の箱を解放し、`_` の位置のフィールドの値を `decref` する。名前を書いた変数は、箱が持っていた参照をそのまま引き継ぐ
  - そうでなければ、名前を書いた位置のフィールドを `dup` してから、x を `decref` する
  - 所有の上では、x の参照が1つ減り、名前を書いた変数の参照がそれぞれ1つ増える
  - 名前は1つ以上書く。1つも残さないときは `decref x` を使う
  - Perceus だけが出す RC の命令である。`verify_scopes` は拒む。`Stmt::defs` は空で、`dup` や `decref` と同じく値の使いとは数えない
- 原子の使い方を「消費」と「読む」に分ける。`switch` の scrutinee と `unpack` の値だけが「読む」で、ほかの使い方 (呼び出し、`apply`、呼ばれる側、extern、`con`、`closure`、`perform`、`resume`、`handle` の引数、`drop`、`return`、`tail`、`jump` の実引数) はすべて「消費」である
- `Lin` の値には `dup` も `decref` も付かない。分解した `Lin` の値はその分解で死ぬので、`Lin` のフィールドをすべて名前で書いた `release` をちょうど1回受ける。型が一意を保つので、共有の側は通らない。IR は Kind を持たないので、これは決まりとして書き、verifier では確かめない
- `con` は引数を1つ以上とる。フィールドのないコンストラクタの値は `#N` (`Atom::Tag`) の1つの書き方にそろえる

### verifier (所有の段)

- 経路ごとに持つのは、今と同じ「RC の対象の変数ごとの所有の数」だけである。R6 (合流で所有が同じ)、R7 (`save` の RC の部分が所有の多重集合と同じ)、「`return` と `tail` で何も所有していない」は変えない
- RC のフィールドの変数には、定義のとき (R5 で1回だけ) 次の2つを記録する
  - 持ち主: 分解した値 s が、束縛の時点で所有の数を1つ以上持っていれば s、そうでなければ s の持ち主
  - 出どころ: `(s, タグ, フィールドの数, 位置)`
- 変数 v が有効なのは、v の所有の数が1つ以上か、v の持ち主の所有の数が1つ以上のときである。1回の参照で決まるので、verifier は線形のままである
  - 健全な理由: 所有の数が正なら、その経路は実際の参照を持ち、物体は生きている。データは変更されないので、その下の物体もすべて生きている
- 規則
  - 分解 (case のフィールド、`unpack`): s は見えて有効な RC の変数である。各 RC のフィールドは所有の数0で始まる。借りた変数への `switch` と `unpack` も受け入れる (この段の Perceus は出さない)
  - 読む (`switch` の scrutinee、`unpack` の値、`dup` の対象): 見えて有効である。`dup v` は v の所有の数を1つ増やす
  - 消費と `decref v`: 見えて、v の所有の数が1つ以上である。1つ減らす。1つの命令の中では左から順に当てる
  - `release x #t(p1..pn)`: 名前が1つ以上ある。x の所有の数が1つ以上である。名前を書いた各 pi は見えて、Repr が RC の対象で、出どころが `(x, t, n, i)` である。その後、x を1つ減らし、各 pi を1つ増やす。1つの位置には1つの名前しか書けないので、同じフィールドを2つの変数が引き継ぐことはない
  - 借りた変数 (所有の数0) は消費できず、`save` の所有の多重集合とも合わないので、呼び出しをまたいで保存されることも、`jump` で渡されることもない。定義が支配する合流のブロックでは見えたままで、持ち主がどの辺でも所有されていれば有効である
  - `verify_scopes` は `release` と、引数のない `con` を拒む
- 持ち主が死んだ後は、間の変数を後で `dup` していても、借りた変数を拒む。保守的だが健全で、この段の Perceus はその形を出さない。親をたどる規則に緩めることは、IR を変えずに後でできる
- 誤りの文言 (テストの期待値)
  - `` `x.1` is {used|released} but is only borrowed from `d.0` `` (所有の数0の有効な変数を消費した)
  - `` `x.1` is {duplicated|switched on|unpacked} after its owner `d.0` was given up `` (無効な変数を読んだ)
  - `` `x.1` is not field 0 of `d.0` #1 `` (`release` の出どころの違い)
  - `` a release of `d.0` keeps no field ``
  - `` `x.1` is kept but is not reference counted ``
  - `` `d.0` is released with its fields before Perceus `` (scope の段)
  - `` a constructor value without fields is written as a tag `#N`, not `con` ``
  - 今の文言 (`after it was moved`、`still owned at the end`、`a call saves .. but owns ..`) は変えない

### Perceus

- 生存解析は「消費」と「読む」の両方を使いとして数える。`dup` が要るかは「消費」の数だけで決める
- case の行き先の入口と `unpack` の直後では、x を scrutinee、L をそこで生きている RC のフィールドとして、次のどれかにする
  - x が死んでいて L がある: L の位置に名前を書いた `release x #t(..)`。2回以上使うフィールドは、その後で今と同じく `dup` する
  - x が死んでいて L がない: `decref x`。フィールドのない case、`default`、文字列の `switch` の行き先を含む
  - x が生きている: L の各フィールドを `dup` する
- その位置での順は、フィールドの `dup`、死んだ変数の `decref` (変数の番号の順)、`release` である。親を先に手放すと入れ子の値の参照が1つになり、`release` が一意の側を通れるためである。変数の番号の順が親から子の順になるのは、translate が束縛の順に番号を振るからである
- 合流の入口と `save` は変えない。呼び出しより前にフィールドはすべて所有になるので、末尾呼び出しの降格は起きない
- 入れ子のパターンでは、フィールドを `dup` してから読むだけの所が残る (UI のプログラムで7か所)。これは、借りた変数への `switch` を使い、フィールドを死ぬ所で手放す形 (遅らせる形) にして取り戻す。ロードマップに残す

### インタプリタとランタイム

- `switch` と `unpack` はヒープを読むだけである。フィールドの値をフィールドの変数に書き、何も取り出さず、複製もしない。文字列の `switch` は `decref` せず、行き先が手放す
- `Heap::release_fields(x, tag, 残す位置)` を足す
  - データでない、タグかフィールドの数が違う: `HeapError::WrongLayout`。機械は内部の誤りとして報告する
  - 一意: 箱を解放し、残さないフィールドの値のうち物体を `decref` する
  - それ以外: 残すフィールドを `dup` してから、x を `decref` する
  - 今ある内部の `Heap::release` (スロットの解放) は `free_slot` に改名する
- `take_or_copy` はクロージャと継続だけを扱う。データ、文字列、ファイルは `NotCopyable` で、その表示は「この物体は複製できない」という意味の文に直す。`copy` もデータと文字列の場合を除く。データを複製しないことを決まりにする
- `RunStats` に `rc_increments` と `rc_decrements` を足し、ヒープの中で数える
  - 参照の数の書き込みを1回と数える。`dup`、`decref` (連鎖する解放とフレームの解放を含む)、`acquire_immortal` が対象である
  - `release` は、x の参照を1つ手放すことを、一意と共有のどちらの側でも1回の減少と数える。共有の側の `dup` と、一意の側で残さないフィールドの `decref` も数える
  - 箱の解放そのものは数えない

### テキストの形

- `release p.0 #0(a.1, _)`。`unpack` と同じく、タグとフィールドごとの項目を書き、使う位置の変数には repr を付けない
- `parse` は、すべて `_` の `release` と、変数でも `_` でもない項目を拒む。往復 (pretty → parse → pretty) は同じテキストに戻る

## 対象外

- S3b-2c の項目 (上の「段の分け方」)
- フィールドのない `#N` の行き先で scrutinee を所有しない規則。`tobj` の値でも即値だと確かめられるように、`switch` に配置の ID が付く S3b-2c で入れる
- reuse (`release_reuse` と `con@token`)、借用パラメータ、遅らせる形のフィールドの規則。どれも IR を作り直さずに足せる形にしてあり、守る約束をロードマップに書く
- 静的に一意と分かる `Lin` の値で、共有の側を省くこと

## テスト

### 足すテスト

- **UI の run テスト** (出力は今の処理系の出力と同じ)
  - `run/data/nullary_target_uses_scrutinee.em`: フィールドのない行き先で scrutinee を使う (`| None -> weight o`)
  - `run/data/nullary_target_before_merge.em`: フィールドのない行き先から合流し、合流の後で scrutinee を使う
  - `run/data/nested_match_parent_live.em`: 親が生きたままの入れ子のパターン
  - `run/effects/multi_shot_takes_saved_data_apart.em`: 保存したデータを、複数回の再開のそれぞれで分解する
- **verifier**
  - 拒む: 借りたフィールドの消費 (`return`、呼び出し、`con`、`jump`、extern、`apply` の引数、呼ばれる側)、`decref`、`save`。`release x` や `decref x` や x の消費の後に、残さなかったフィールドを `dup` か `switch` する。孫を名前に書いた `release`。出どころの違い。ほかの分解のフィールド。支配しない枝のフィールド。RC でない名前。二重の `release`。借りた x の `release`。`con #N()`。scope の段の `release`
  - 受け入れる: 入れ子の `release`。どの辺でも x が所有されているときに、合流の後で使う借りたフィールド。持ち主の `decref` の前に `dup` したフィールド。持ち主が所有されている借りたフィールドへの `switch` と、内側のフィールドの `dup`。2回所有した x の `release` (その後も x は所有されている)。Perceus が出す `label` と `| Nil -> xs` の形
  - 2万の深さの、借りたフィールドへの `switch` の連鎖が線形の時間で通る
- **Perceus のスナップショット**: `unpack` の後の `release`。case の入口でフィールドの一部を残す `release`。2回使うフィールド (`release` の後の `dup`)。生きている scrutinee (フィールドの `dup` だけ)。フィールドを使わない死んだ scrutinee (`decref`)。scrutinee が生きているフィールドのない行き先。`default` の行き先。行き先が scrutinee を使う文字列の `switch` (`switch` の前に `dup` しない)。入れ子のパターン。呼び出しをまたいで保存するフィールド。`read_file` の形 (`File` に `dup` も `decref` も付かない)。入口での順
- **テキスト**: `release` の往復。すべて `_` の `release` を読まない
- **eml_runtime**: `release_fields` の一意の側 (箱を解放し、残さないフィールドを `decref` し、残すフィールドは変えない)、共有の側、同じ値が2つの位置にある場合、`File` のフィールドを残す場合と残さない場合 (閉じる)、不死の値、データでない値、タグと数の違い、解放後の使用
- **eml_interp**: `debug_heap` 付きの `release` (一意、共有、同じ値が2つの位置にある場合)。検証しない IR での `release` の内部の誤り
- **スケーリング** (`eml_interp/tests/scaling.rs`): 一意なリストを長さ n と 2n でたどり、`rc_increments` が長さによらず同じであること。共有されたリストでは `rc_increments` が n + 定数以下であること。この2つは今の main でも通る。消費しない形にしたのに `release` を入れなかったときの後退を捕まえる見張りである

### テストの変更

[テスト戦略](../../implementation/testing.md) の「テストの変更の運用」の種類で書く。どれも試作で確かめた。

**成否の変更 (テストの削除)**

- `eml_core_ir/tests/verify.rs`
  - `an_unpack_consumes_its_value_and_owns_its_fields`: 3つの検査がすべて変わり、名前が古い規則を言う。上の「足すテスト」の verifier のテストに置き換える
  - `an_unused_field_must_be_released` (補助の `unused_field` も): 借りたフィールドの `decref` は誤りになる
  - `a_scrutinee_used_in_an_arm_is_duplicated_before_the_switch` (補助の `keep_scrutinee` も): `switch` の前の `dup` は所有が多すぎる
- `eml_core_ir/tests/perceus.rs`: 次の5件は古い規則を名前にしている。新しい規則のスナップショットに置き換える
  - `an_unpacked_value_used_later_is_dupped_before_the_unpack`
  - `a_scrutinee_used_by_a_target_is_dupped_before_the_switch`
  - `an_unused_field_is_decreffed_when_its_arm_starts`
  - `a_scrutinee_used_in_an_arm_is_dupped_before_the_switch`
  - `a_string_switch_dups_a_scrutinee_that_an_arm_uses`
- `eml_interp/tests/data.rs`
  - `a_unique_value_is_unpacked_by_taking_its_fields` と `a_shared_value_is_unpacked_by_copying_its_fields`: `release` の一意と共有のテストに置き換える
  - `a_string_switch_releases_the_string_on_every_path`: 行き先が手放すので、名前を変えて足し直す (出力は同じ)
  - `an_unpack_takes_or_copies_the_fields`: `unpack` は読むだけになるので、名前を変えて足し直す (出力は同じ)
- `eml_runtime/src/heap/tests.rs` (`take_or_copy` がデータと文字列を扱わなくなるため)
  - `take_or_copy_takes_a_unique_data_object_with_its_fields`、`take_or_copy_copies_a_shared_data_object_and_dups_its_fields`、`take_or_copy_copies_an_immortal_string`: 削除する
  - `take_or_copy_takes_a_unique_object`: クロージャで書き直す
  - `string_bytes_written_counts_the_contents_of_string_objects`: `take_or_copy` の手順を `append_str` に置き換え、検査する数が変わる

**期待値の変更 (範囲)**

- `eml_core_ir/tests/perceus.rs` の `an_unused_unpack_field_is_released_after_the_unpack` と `a_field_passed_to_an_arm_and_used_after_it_is_dupped_before_the_jump` のスナップショット
- `eml_core_ir/tests/verify.rs` と `eml_interp/tests/data.rs` の手書きの IR を、新しい所有の規則 (行き先が scrutinee を `decref` か `release` し、フィールドを `decref` しない) に書き直す。検査する値は変えない。対象は `a_field_is_in_scope_in_the_blocks_its_arm_dominates`、`a_switch_that_binds_fields_is_accepted`、`a_tobj_variable_holding_a_tag_takes_its_case`、`a_value_with_fields_that_goes_to_the_default_is_released` である

**機械的な追随**

- `tests/ui/run/data/shared_scrutinee.em` と `tests/ui/run/data/string_literal_default_uses_the_value.em` の先頭のコメント、`unpack_of_tag_one` のコメント、perceus.rs の古い規則を書いたコメント。出力は変わらない
- `a_shared_file_is_not_copied` のコメント (ファイルは参照の数によらず複製できない)

## 確認の手順

- `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check`
- 既定でない feature の組み合わせ: `cargo clippy -p eml_cli --all-targets --no-default-features` と `--features types`、`--features core`。`eml_test_support` の同じ組み合わせと `--features hir`
- `cargo test -p eml_cli --test integration citations`
- `nix build`

## 更新する文書

段の終わりに直す。

- `docs/spec/core-ir.md`: 文の表と RC の命令 (`release`)、フィールドの束縛の規則 (借りたフィールド)、S3b-2b を指す記述を S3b-2c に向け直す (`TailCall` の降格、R8 の呼び出しの結果、多相な位置)、`Lin` の決まり、Perceus の規則 (消費と読む、入口の規則と順)、`verify_scopes` (`release` と引数のない `con`)、所有の検査 (持ち主と出どころ)、移動の原則が「読む」使いを名指しすること、`switch` と `unpack` の意味、位置のない実行時エラーに `release` を足すこと
- `docs/spec/runtime.md`: `Lin` の記述、不死の物体と操作の一覧 (`release_fields`、`take_or_copy` の範囲)、ランタイムの API、`match` が `take_or_copy` を使わないこと、`RunStats` の数
- `docs/implementation/architecture.md`: 原子の使い方の区別、機械の `release`、RC の数を数える場所
- `docs/implementation/testing.md`: ヒープの単体テストの一覧、テキストの形 (`release` の項目、使う位置)、スケーリングのテストと数
- `docs/future/roadmap.md`
  - 段の表の S3b-2b の行と節を「S3b-2b Core IR v2 の所有」と「S3b-2c Core IR v2 の表現」に分ける。S4 の前提は S3b-2c にする。S3b-2b の節は段の終わりに削除する
  - S3b-2b の論点 (`Lin` の scrutinee、一意な箱での RC の操作の数、呼び出しをまたぐ借用、定数の scrutinee) を閉じる
  - `| ys -> f ys` の項目を消す
  - 「Perceus の最適化」に次を書く: 借用パラメータの約束 (1つの呼び出しでは所有の引数を消費した後で借用の引数を確かめる、`save` に借用の部分を足す、借用のブロック引数は持ち主を宣言する)、reuse のトークンを `save` に入れないこと、借用の推論で「同じ配置の `con` を作る行き先の scrutinee」を所有にすること、フィールドを死ぬ所で手放す遅らせる形
- `docs/overview.md` の段の一覧、`docs/README.md`、`CLAUDE.md` (Core IR の文の一覧)、[全体設計](2026-10-07-redesign-design.md) の S3b の記述
- コードのコメント: Perceus、verifier、機械、ランタイム、ヒープで古い規則を書いた所

最後に、`grep -rn -e take_or_copy -e 'S3b-2b' docs CLAUDE.md crates` が、意図して残す記述だけを出すことを確かめる。

## 完了の条件

- UI テストの出力が変わらない (足す4本を除く)
- `switch` と `unpack` が scrutinee を消費せず、データが複製されない
- 一意なリストをたどる `rc_increments` が長さによらない
- 上の確認の手順がすべて通る
