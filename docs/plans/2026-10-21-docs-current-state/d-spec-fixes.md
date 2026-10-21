# 付録 D: spec の食い違いの直し (S4)

計画書 plan.md が正である。この付録と plan.md が食い違うときは plan.md に従う。W-D12 は末尾の「W-D12 (置き換え)」が正で、本文の W-D12 は使わない。

# S4: spec と実装・ほかの文書の食い違いの直し方と作業の分け方 (r2)

対象は 0fff343a の時点のリポジトリである。行番号は調査ファイルのもので、少しずれている所がある。実装者は行番号でなく、節の名前と、作業ごとの確かめの検索の形で探す。

記号: [V] は確かめた事実 (方法を添える)、[I] は推測。「テストの変更」の欄には、3種類 (成否 / 期待値 / 機械的) のどれか、または「なし」を書く。

## 0. S1 との関係

- S4 のどの作業も、S1 の最初の単位「決まりの文書化」のコミットの後に置く。その単位が入れる決まり (docs/ は今の状態を書き、段の名前、日付、前の状態との対比、決定の記録を書かない。数値は benchmarks.md だけが持つ) に従って書く
- [H] と同じ文を直す項目 (表の「[H] と同じ文」の印) は、段の名前、日付、「今までどおり」「まだ」「後で」を書かず、今の状態だけを書く。これは S1 の原則の細部がどう決まっても成り立つ。理由を残すときは「〜にすると〜になるため」の形にする
- 数を書いていた所は、測った値を書き直すのでなく、数を書かない形にする (S1 の「数値は benchmarks.md だけが持つ」に合わせる)。テストの大きさのような設定の値 (2000 と 8000 など) は数値の記録でないので残す
- S1 と同じファイルでは、S1 が持たない箇所だけを直す。S1 が持つ箇所は次のとおりで、S4 は触らない
  - docs/README.md: 「文書の扱い」と、地図の status、roadmap、benchmarks、plans の行
  - docs/implementation/testing.md: 「テストの変更の運用」と「文書の引用の検査」
  - docs/future/roadmap.md: 「進め方」
  - CLAUDE.md: Rules と Testing の節
- S4 が触る S1 のファイルの箇所: README の地図のほかの行の「内容」、testing.md の「期待値の表」「Miri」「`eml_vm` のテスト」「LIR のテキストの形」「分類」「性能のテスト」と :51、:53、:58、architecture.md の「リポジトリ」「`eml_core_ir`、`eml_lir`、`eml_vm` の内部」「CLI と lib API」、CLAUDE.md の Architecture の節の E0004 の1文
- [H] だけの文 (段の名前を含むが、食い違いの直しと関わらない文) は S4 の項目にしない。たとえば testing.md:84 の「773 verified …約 20 秒 (S9b-3 の終わり)」、overview.md:96 の用語表、overview.md:64

## 1. 結論

### a1 (構文)

| 項目 | 事実か | 直す側 | 直しの中身 | テストの変更 | 聞くか |
|---|---|---|---|---|---|
| a1-1 lexical.md「演算子」の区切り記号の `\` | 事実 [V] lexer の `Raw` に `\` がなく、grammar.md に `\` を使う規則もない | spec | 区切り記号の列から `\` を除く | なし | いいえ |
| a1-2 lexical.md「空白とコメント」の空白の定義 | 事実 [V] 空白は `[ \t\r\n]+`。E0006 は行頭の字下げのタブだけ。単体テスト `tab_after_the_first_token_is_fine` が行の途中のタブを意図して受け付ける。単独の `\r` はレイアウトの改行にならない (`x : Int\rx = 1` は1行として読まれ、E1005 と E1015 になる) が、ariadne の表示は `\r` で行を分けて2行に見せる | spec | 空白を「スペース、タブ、`\r`、`\n`」にする。改行は `\n` (`\r\n` を含む) で、単独の `\r` は空白だが改行ではない。E0006 になるのは行頭の字下げのタブだけで、行の途中のタブはレイアウトの列に影響しない (列を読むのは行の先頭のトークンだけ) | なし | 単独の `\r` を聞く (Q1) |
| a1-3 `--|` などが演算子になること | 食い違いではない | 直さない | | なし | いいえ |
| a1-4 lexical.md「数値と文字」の E0007 | 事実 [V] `0XFF` と `1abc` は E0007 `invalid number literal` | spec | 不正な形の数 (大文字の接頭辞 `0X`/`0O`/`0B`、`1abc` のような数の直後の英字、`0xZZ`) も E0007 であることを足し、接頭辞は小文字だけと書く | なし | いいえ |
| a1-5 閉じていないブロックコメントの E0005 | 事実 [V] `{- open` で E0005 | spec | ブロックコメントの項に「閉じていなければ E0005」を足す | なし | いいえ |
| a1-6 grammar.md「文法上の補足」の `let` の関数の形の判定 | 事実 [V] `at_apat_start_at` は `FLOAT` と `{` も数える | spec | 列挙に浮動小数と `{` を足す。浮動小数は `apat` の `literal` で、HIR が E1052 にする。`{` は、同じ節の「コンストラクタのない `{` は…パターンの位置ではパターンを期待する誤りにする」に合わせて数える。コードで `{` を外す案は退けた (3) | なし | いいえ |
| a1-7 入れ子の深さ 256 | 食い違いではない | 直さない | | なし | いいえ |
| a1-8 layout.md の規則 2 と 5 のエラーの番号 | 事実 | spec | 規則 2 と規則 5 の「エラーとする」に「(E0011。parser の閉じ括弧の検査が出す)」を足す | なし | いいえ |
| a1-9 フィールドのセクション `(.name)` と E2013 | 事実 [V] | spec | expressions.md の「セクション」の `(.f)` の項に、引数の型が先に決まらなければ E2013 になること (`List.map (.name) people` のように期待する型から決まる位置では通る) を書き、types.md の「フィールドの解決」を引く。records.md は触らない (事実を1か所に置く) | なし | いいえ |
| a1-10 examples.md:5「標準ライブラリの API はまだ設計していない」([H] と同じ文) | 事実 | spec | 冒頭の段落を今の状態だけで書く。「標準ライブラリの API は [標準ライブラリ](stdlib.md) が定める。今の実装で動く例は `tests/ui/run/scripts/` の5本 (`wc`、`grep`、`logstat`、`csv2jsonl`、`deploy`)。ここの例には、今の std にない関数 (`Fs.read_lines`、`Proc.spawn` など) と、実装していない構文 (コマンドリテラル) を使うものがあり、構文を示すためのものである」の内容にする。「議論しながら書いた」「S12 で書いた」と、roadmap の「段の列」への参照は書かない | なし | いいえ |
| a1-11 examples.md の例が今の std にない名前を使う | 事実 | 直さない | a1-10 の断り書きの範囲 | なし | いいえ |
| a1-12 自前の `Fail` と std の `Fail` | 衝突しない | 直さない | | なし | いいえ |
| a1-13 future/stdlib.md:1-3「API の設計は別の spec で扱う」 | 事実 | ほかの文書 | 「標準ライブラリの規範は [標準ライブラリ](../spec/stdlib.md) にある。この文書は、まだ入れていない API への申し送りをまとめる」の内容にする | なし | いいえ |

### a2 (宣言、式、モジュール、REPL)

| 項目 | 事実か | 直す側 | 直しの中身 | テストの変更 | 聞くか |
|---|---|---|---|---|---|
| a2-1 repl.md「値の書き方」の例の `Int -> Int` | 事実 [V] `fn x -> x + 1` は `<fn> : Int -> <Panic> Int` | spec | 例のブロックを、トランスクリプトのスナップショット `crates/eml_cli/tests/snapshots/integration__repl__transcripts@basics__displayer.repl.snap` の行だけで組み直す。使う行 (この順): `> inc : Int -> <Panic> Int`、`. inc x = x + 1`、`.`、`> inc`、`<fn> : Int -> <Panic> Int`、`> fn x -> x * 2`、`<fn> : Int -> <Panic> Int`、`> data Box = | Box (Int -> <Panic> Int)`、`> Box inc`、`Box <fn> : Box`、`> [fn x -> x, inc]`、`[<fn>, <fn>] : List (Int -> <Panic> Int)`、`> data R = | R { f : Int -> <Panic> Int, n : Int }`、`> R { f = inc, n = -3 }`、`R { f = <fn>, n = -3 } : R`。例の `> 1 + 2` / `3` は消す (スナップショットにない。`Show` を持つ値を値だけで書くことは、例の前の箇条書きが述べている)。行を書き換えたり足したりしない | なし | いいえ |
| a2-2 `P { f = inc, n = 2 }` が E2001 になる | 事実 [V] | spec | a2-1 の組み直しで消える | なし | いいえ |
| a2-3 `Outcome` の表に `Exited` がない | 事実 | spec | 表に `Exited(n)` の行を足す。表示は「割り込みと実行時エラー」の節の記述 (標準エラーに `exited with code n`) に合わせ、その節を引く | なし | いいえ |
| a2-4 repl.md「上の「defaulting しない」」 | 事実 (見出しでない) | spec | 「上で述べた、型変数を `Unit` に決めない規則」のように、見出しの引用に見えない書き方にする | なし | いいえ |
| a2-5 modules.md「下の「`import Std.X as F` で標準ライブラリに届く」」 | 事実 (見出しでない) | spec | a2-4 と同じ扱い | なし | いいえ |
| a2-6 modules.md:15 の `exit` の参照先が future | 事実 | spec | 参照先を [標準ライブラリ](stdlib.md) の「`exit`」にする | なし | いいえ |
| a2-7 modules.md の E1031 の help | 事実 [V] 別名の import があれば ``the import of `Std.String` gives the qualifier `S`; write `S.length` ``、import がなければ ``write `String.length`, or `import Std.String as S` `` | spec とほかの文書 | 「標準ライブラリ」の手順 4 の help を2つの場合に分ける。docs/implementation/diagnostics.md の E1031 の行の最後の文 (「`Fs.close` と書く」) も同じ2つの場合にする | なし | いいえ |
| a2-8 `deriving (Foo)` | 事実 [V] E1002 `cannot find class Foo` | spec | declarations.md「`deriving`」の E1038 の文の後に「名前がクラスに解決しなければ E1002」を足す | なし | いいえ |
| a2-9 `instance C (Box a a)` が E1015 と E1039 を出す | 事実 | 直さない | 個数の誤りと型変数の重複は独立した違反で、どちらの規則も spec にある | なし | いいえ |
| a2-10 `Box x == Box y = …` が E0011 | 事実 | 直さない (この作業では) | 意図した制限ではない。status.md は、演算子の等式の先読み (`at_operator_equation`) が単純なパターンを1つしか認めないための「既知の制限」と書き、CLAUDE.md の Syntax は Haskell の慣習に寄せる (Haskell ではこの形が普通)。直すのは文法を広げる作業 (grammar.md の `equation` の `apat OP apat` と先読みの両方を変える) で、食い違いの直しの範囲を超える。課題の記録は status.md の「既知の制限」にすでにあり、S2 がそこか roadmap の項目に残す | なし | いいえ |
| a2-11 `data T <e> e` で E1003 の後に E1002 | 事実 | 直さない (この作業では) | 連鎖の抑止が足りない診断の課題で、status.md の「既知の制限」にある。spec は E1003 と最初の定義を残すことを定めるだけで、食い違いではない | なし | いいえ |
| a2-12 declarations.md の Prelude の instance の表 | 事実 [V] `std/Prelude.em` に `instance Num Int`、`Div Int`、`Num Float`、`Div Float`、`Eq Float`、`Ord Float`、`Show IOError` と、`IOErrorKind` の `deriving (Eq, Show)` がある | spec | 表に `Num Int`、`Div Int`、`Num Float`、`Div Float`、`Eq Float`、`Ord Float`、`Eq IOErrorKind`、`Show IOErrorKind`、`Show IOError` を足す (1行に複数をまとめてよい)。`Num`/`Div` と `Eq Float`/`Ord Float` の「定義」は本文の該当の項を引く | なし | いいえ |
| a2-13 declarations.md の操作のシグネチャの E1007 | 事実 [V] `op : Int` は E1007 | spec | 「書いたらエラー」を「E1007」にし、シグネチャが関数型でない場合も E1007 であることを足す | なし | いいえ |
| a2-14 README の expressions.md の行 | 事実 | ほかの文書 | a7-2 で直す | なし | いいえ |
| a2-15 declarations.md:338「リストを扱う関数 (`map`、`length` など) は、まだ Prelude にない」([H] と同じ文) | 事実として正しいが、`List` のモジュールにあることを書かない | spec | 「リストを扱う関数は Prelude になく、`List` のモジュールにある ([標準ライブラリ](stdlib.md) の「`List`」)」の内容にする。「まだ」は書かない | なし | いいえ |

### a3 (型、線形性、エフェクト、網羅性、診断)

| 項目 | 事実か | 直す側 | 直しの中身 | テストの変更 | 聞くか |
|---|---|---|---|---|---|
| a3-1 effects.md:79 の `RunConfig::file_root` | 事実 [V] `RunConfig` は `debug_heap` と `control` だけ | spec | 「Host の基準ディレクトリ (`file_root`。[オブジェクトモデル](object-model.md) の `StdHost` の項) から解釈する」にし、既定 (空のパス) の文も Host の `file_root` の話にする | なし | いいえ |
| a3-2 diagnostics.md「データ構造」の「構文、HIR、型検査のどの段階でも同じ意味で使うので」 | 半分は事実 [V] E0004 を出すのは eml_hir だけ (`Diagnostic::not_yet_supported` の呼び出しは eml_hir の6か所だけ) | spec | 理由を「どの段階が出しても同じ意味なので」にする (architecture.md:174 と同じ言い方) | なし | いいえ |
| a3-3 diagnostics.md:160「HIR 以降の段階は…見つけた段階で E0004 を出して回復する」 | 食い違いではない | 直さない | 規則として正しい。同じ段落の「コマンドリテラルの段」は [H] だけの語で、S4 の項目にしない | なし | いいえ |
| a3-4 CLAUDE.md の Architecture の "E0004 (not yet supported) is used by every stage" | 事実 [V] | CLAUDE.md (S1 が持たない Architecture の節) | "E0004 (not yet supported) means the same at every stage, so it lives in `eml_diagnostics`" にする。規則 (置き場所) は変えず、理由の事実の誤りだけを直す | なし | はい (Q2) |
| a3-5 eml_syntax/src/lib.rs:27 のコメント | 事実 | コード (コメント) | 「どの段階が出しても同じ意味なので」にし、docs/spec/diagnostics.md の「データ構造」を引く | なし | いいえ |
| a3-6 implementation/diagnostics.md の E1017 の行 | 事実 [V] 行にローカル関数の組はあるが、ローカル関数の引数の並びがない。`let g a a = a` は E1017 | ほかの文書 | 並びの列挙に「ローカル関数の引数の並び」を足す | なし | いいえ |
| a3-7 同 E4003 の行 | 事実 [V] `let k (Some y) = y` は E4003 | ほかの文書 | 「ローカル関数の引数」を足す | なし | いいえ |
| a3-8 同 E1022 の行 | 事実 | ほかの文書 | 「REPL では、この入力で定義していない演算子への宣言で、文言も「この入力」と言う」を spec の行から写す | なし | いいえ |
| a3-9 types.md「Kind」の `Unit` の分類 | 事実 [V] Prelude は `pub extern data Unit` と宣言し、`eml_types/src/shape.rs:189` がその extern の型を要素のないタプル (`store.rs:275`) に写す | spec | extern の型の Kind の項の「`Int`、`Float`、`String`、`Unit` は `Unr`」から `Unit` を外し、「`Unit` は Prelude が extern の型として宣言する名前で、型としては要素のないタプルである (下の「直積型と Unit」)」を足す | なし | いいえ |
| a3-10 types.md「関数型」の `main` の row のラベルの重複 | 事実 [V] `main : Unit -> <Panic, IO, Panic> Unit` は `run` まで通る | spec | `main` の項 (「row のラベルは `IO` と `Panic` だけ」) に「同じラベルを重ねて書いてもよく、1つにまとめる」を足し、同じ節の重複をまとめる規則を引く | なし | いいえ |

### a4 (Core IR)

| 項目 | 事実か | 直す側 | 直しの中身 | テストの変更 | 聞くか |
|---|---|---|---|---|---|
| a4-1 公開名 `eml_core_ir::type_repr` が `boxed_repr` の別名 | 事実 [V] `translate/mod.rs:41` `pub use types::boxed_repr as type_repr;`。spec の `type_repr` は crate の中の `FnLowering::type_repr` (構造体を決める側) を指し、正しい | コード | 別名をやめ、`boxed_repr` を同じ名前で公開する (`translate/mod.rs` と `lib.rs:30`)。`crates/eml_core_ir/tests/externs.rs` の `use` と3か所の呼び出しを `boxed_repr` にする。extern の行は箱の位置なので、このテストが比べたいのは `boxed_repr` である | 機械的 | いいえ |
| a4-2 architecture.md:281 と testing.md:58 | 事実 [V] `types.rs` に `repr` はない | ほかの文書 | architecture.md: 「変数の Repr は `translate/program.rs` の `ProgramBuilder::repr` が決め (`FnLowering::type_repr` がこれを呼ぶ)、箱の位置の Repr は `types.rs` の `boxed_repr` が決める。`boxed_repr` を `eml_core_ir::boxed_repr` として公開し、extern の表との結び付けのテストが使う」。testing.md:58 の `eml_core_ir::type_repr` を `eml_core_ir::boxed_repr` にし、「translate の型から Repr を決める規則」を「箱の位置の Repr を決める規則」にする | なし | いいえ |
| a4-3 core-ir.md「translate の配置」の extern が作る `Bool` | 事実 [V] `Prelude.shows_minus` と `Std.Float.can_parse` も `Enum` を返す。`eml_extern` の `ret` の doc コメントは正しく列挙している | spec | 列挙に `Prelude.shows_minus` と `Std.Float.can_parse` を足す | なし | いいえ |
| a4-4 core-ir.md「REPL の入力」の `Shown::Extern` | 事実 | spec | 式の入力の扱いに4つ目の場合を足す。入力の値が `Show` を持たない extern の型なら `<extern>` を書く | なし | いいえ |
| a4-5 core-ir.md「IO の extern」の `Panic` | 事実 [V] エフェクトの番号は `eml_hir::Program::effects` の順 (モジュールの番号の順、モジュールの中の宣言の順) での位置である (`translate/program.rs:749-760` の `core_effects` と `effect_index`)。Prelude は `IO` の次に `Panic` を宣言する (`std/Prelude.em:37`、`:50`)。translate の出力も `Prelude.IO`、`Prelude.Panic` の順 (`tests/translate.rs:12-13`) | spec | 「`IO` と `Panic` は Prelude の最初の2つのエフェクトとして、いつもエフェクトの表にある。番号は 0 と 1 である」にする。「変換の規則」で extern のエフェクトを `IO` だけで書く2か所に `Panic` を足す。`core_effects` の doc コメントの「`IO` は…いつも 0 番」にも「`Panic` は 1 番」を足す | なし | いいえ |
| a4-6 core-ir.md「上の「RC の対象でない値の印は `unr`」」 | 事実 (見出しでない) | spec | a2-4 と同じ扱い | なし | いいえ |
| a4-7 core-ir.md 冒頭の共通の規則に「構造体の形」がない | 事実 | spec | 冒頭の列に「構造体の形」を足し、lir.md:5 の列にそろえる | なし | いいえ |

### a5 (LIR)

| 項目 | 事実か | 直す側 | 直しの中身 | テストの変更 | 聞くか |
|---|---|---|---|---|---|
| a5-1 lir.md のアトムの列挙に `Float` の定数がない | 事実 | spec | 「値 (アトム) は」の列挙に `Float` の定数 (ビット列 `FloatBits` で持つ) を足す | なし | いいえ |
| a5-2 lir.md「分解のときの `release` だけになる」と構造体の箱の `drop` | 事実 [V] `perceus.rs:238-250` | spec | 文を「分解のときの `release` と、`unr` でない構造体の箱を手放す `drop` だけになる」にし、「`drop` は消費なので何も足さない」の対象を「EP と translate が出した `drop`」に限る | なし | いいえ |
| a5-3 lir.md の「出口の生存集合」と `live_after_term` | 事実 | spec | `switch` と `jump` の2つの項の「出口の生存集合」を「終端の直前の生存集合 (終端が使う変数を含む)」と呼び、`live_after_term` を「それから終端が使う変数を除いたもの」と書く | なし | いいえ |
| a5-4 lir.md「Perceus が足すのは、構造体の箱を手放す `drop` の結果の `unit` の変数だけ」 | 事実 | spec | 「LIR のパスが足す変数は、Perceus の構造体の箱の `drop` の結果の `unit` の変数と、reuse の token の変数 (`r`。下の「reuse」) だけである」にする | なし | いいえ |
| a5-5 lir.md「Perceus はそこで `File` を `dup` する」([H] と同じ文) | 事実 (V1 と食い違う仮定の文) | spec | この文と、続くロードマップへの参照を消す。「この議論は、決定木の途中にガードがないことに依る」で止める | なし | いいえ |
| a5-6 lir.md「実行時エラー」にリークと回復の失敗の表示がない | 事実。vm.md「誤り、止めること、終わり」に定義がある | spec | 「リークの表示は位置を持たない」の文に「リークと回復の失敗の表示の形は [VM](vm.md) の「誤り、止めること、終わり」にある」を足す | なし | いいえ |
| a5-7 testing.md「LIR のテキストの形」に `token` がない | 事実 | ほかの文書 | repr の列挙に `token` を足し、`let r: token = release x L #t(..)`、`con L #k(=a1, a2) reuse r [in place]`、`drop_token r` の形を足す。中身は lir.md の「reuse」を引き、`crates/eml_lir/tests/reuse.rs` のテキストから写す | なし | いいえ |
| a5-8 testing.md:51 の `contract_text` と `perceus_text` の出力側の段 | 事実 | ほかの文書 | 「`contract_text` は入力と出力を範囲の段で、`perceus_text` は入力を範囲の段、出力を所有の段 (`verify`) で確かめる」にする | なし | いいえ |
| a5-9 architecture.md:276「verifier が定数で分ける `switch` を拒む」 | 事実 [V] lir.md の R8 の補足に「タグの `switch` の scrutinee は変数で…」がすでにあり、`check.rs:712-717` はそれを確かめる | ほかの文書だけ | architecture.md の「定数で分けることになる `switch`」と「verifier が定数で分ける `switch` を拒む」を、タグの `switch` に限る言い方にし、lir.md の R8 (「データの配置」の構造の規則) を引く。lir.md は直さない | なし | いいえ |
| a5-10 docs/superpowers の古い参照 | 範囲外 | 直さない | | | |
| a5-11 lir.md の Repr の列挙と `token` | 食い違いではない | 直さない | | | |

### a6 (VM、オブジェクトモデル、標準ライブラリ)

| 項目 | 事実か | 直す側 | 直しの中身 | テストの変更 | 聞くか |
|---|---|---|---|---|---|
| a6-1 vm.md「上限と割り込み」の `unsafe` の置き場所 | 事実 (コードは3か所。architecture.md:354 はコードと合う) | spec | 「`eml_cli` の `src/` の `unsafe` は、この関数と、bin の `sigint.rs` と、`terminal.rs` の `stdout_width` の中だけに置く」にする | なし | いいえ |
| a6-2 vm.md「置き場所」の再公開の一覧に `Finished` がない | 事実 (`crates/eml_cli/src/lib.rs:20-23`) | spec | 一覧に `Finished` を足す | なし | いいえ |
| a6-3 vm.md「スロットとスタック」の VM の仕事の回数の列挙 | 事実 [V] `RunCounts` は5欄 | spec | 列挙に `conversions` と `reuses` を足す | なし | いいえ |
| a6-4 同じ文の `Task` の欄 | 事実 | spec | `stack_limit`、`entry`、`keep_result`、`exit` を足す。役目は `crates/eml_vm/src/vm.rs:16-34` の doc コメントから1句ずつ取り、`keep_result` は「追記」、`exit` は「`exit`」の節を引く | なし | いいえ |
| a6-5 `eml_vm::compile_error` が vm.md にない | 事実 | spec | 「`CodeBase` と `FnEntry`」の `CompileError` の段落に、`compile_error(program, error)` が `CompileError` を内部の誤りの `RuntimeError` にすること、`execute` と REPL の `Machine::load` の失敗 (`Outcome::Broken`) がこれを使うことを足す | なし | いいえ |
| a6-6 `CompileError` の列挙に `NoConversion` がない | 事実 [V] `code.rs:120-135` | spec | 列挙に `NoConversion` (「変換のフレーム」で変換を作れない組) を足す | なし | いいえ |
| a6-7 「行の `purity` によらず」 | 事実 [V] `failed` は `fault.panics() && purity != Purity::Pure` | spec とコード (コメント) | vm.md「`IO` と `Panic` への中断」と stdlib.md「`try_panic`」の2か所を「`Pure` でない行の誤りのうち `Fault::panics` が真のものを `Panic` に届ける。`Pure` の行は巻き戻しの地点を持たないので届けない」にする。`crates/eml_vm/src/vm.rs:999` の `failed` の doc コメントも同じ内容にし、vm.md の節を引く。同じコメントの「今までどおり実行時エラーにする」は「実行時エラーにする」にする | なし | いいえ |
| a6-8 `eml run` の `runtime error: ` の接頭辞 | 事実 (`crates/eml_cli/src/main.rs:88`) | spec | vm.md「誤り、止めること、終わり」の表示の形の項に「`eml run` は表示の前に `runtime error: ` を付けて標準エラーに書く」を足す | なし | いいえ |
| a6-9 object-model.md「誤り」の `ArithFault` | 事実 [V] `DivisionByZero`、`IntegerOverflow`、`FloatToInt(u64)`、`FixedDigits(i64)` | spec | 種類を4つにし、`Arith(ArithFault)` の表示に ``float `{}` does not fit in `Int` `` と ``` `to_fixed` takes 0 to 100 digits, not {n} ``` を足す (文言は `eml_rt/src/externs.rs` の `Display` から写す) | なし | いいえ |
| a6-10 object-model.md「extern」の 83 個の表に配列の11行がない | 事実 [V] 表の関数は 72 個 | spec | 表に1行足す。行は `Std.Array.make`、`length`、`get`、`set`、`thaw`、`Std.MutArray.make`、`empty`、`length`、`read`、`write`、`freeze`、関数は `Ctx::array_make`、`array_empty`、`array_length`、`array_get`、`array_set`、`array_freeze`、`array_thaw` (上の「配列」) | なし | いいえ |
| a6-11 `unit` の引数を持つ行に `MutArray.empty` がない | 事実 | spec | 列挙に `Std.MutArray.empty` を足す | なし | いいえ |
| a6-12 stdlib.md「`try_panic`」の `Panic` を起こす関数の表 | 事実 [V] `Fault::panics` は `Arith(_)` と `Index(_)` の全部で真 | spec | `Arith(_)` の行に `Float.truncate`、`Float.floor`、`Float.ceil`、`Float.round` (`Int` に収まらない値) と `Float.to_fixed` (桁数) を足す。`Index(_)` の行に `Array.get`、`Array.set`、`Array.make`、`MutArray.read`、`MutArray.write`、`MutArray.make` (添字の範囲と負の長さ) と、添字の式が使う `check_index` を足す。名前はこの形 (モジュール名付き) で書く。実装者は各関数の std の宣言の row に `Panic` があることを確かめる | なし | いいえ |
| a6-13 effects.md「組み込みの `Panic`」の起こしうる関数の列挙 | 事実 | spec | 列挙を例 (`Num` と `Div` のメソッド、`String.byte_at`) にとどめ、「一覧は [標準ライブラリ](stdlib.md) の「`try_panic`」の表にある」を足す (事実を1か所に置く) | なし | いいえ |
| a6-14 status.md、README、testing.md、future/stdlib.md | | status.md は S2。ほかは a7 で直す | | | |

### a7 (手引き、将来の設計、CLAUDE.md)

| 項目 | 事実か | 直す側 | 直しの中身 | テストの変更 | 聞くか |
|---|---|---|---|---|---|
| a7-1 README の roadmap の行 | 範囲外 (S1) | 直さない | | | |
| a7-2 README の地図の「内容」の欄の欠け | 事実 | ほかの文書 | 下の「足す語の表」の語を、各行の「内容」の欄に足す。欄の既存の語は消さない。status、roadmap、benchmarks、plans の行は触らない | なし | いいえ |
| a7-3 docs/superpowers が地図にない | 範囲外 | 直さない | | | |
| a7-4 overview.md:96、:64 | 事実 | 直さない (S4 の項目にしない) | [H] だけの文 (0 を参照) | | |
| a7-5 overview.md「確定した設計判断」に `Panic`、`var`、数、配列がない | 事実 [V] `grep` で0件 | ほかの文書 | 表に4行を足す。`Panic` (値と入力で決まる誤りを extern のエフェクト `Panic` にし、`try_panic` で捕まえる。effects.md「組み込みの `Panic`」)、`var` (局所の可変の変数。expressions.md「`var`」と effects.md「`var` のエフェクト」)、数 (`Int` と `Float`、クラス `Num` と `Div`。types.md「数の型」)、配列 (`Array` と `MutArray`。stdlib.md「`Std.Array`」「`Std.MutArray`」) | なし | いいえ |
| a7-6 architecture.md「CLI と lib API」 | 事実 | ほかの文書 | `eml FILE ARGS...` (`run` を省く形)、`eml run FILE` の後ろの引数をスクリプトに渡すこと (`split_script_args`)、`script_host`、`eml repl [file]` を足し、規範は stdlib.md の「スクリプトの起動」を引く | なし | いいえ |
| a7-7 architecture.md の木と各 crate のファイルの列挙 | 事実 | ほかの文書 | 「リポジトリ」の木に `bench/`、`Cargo.lock`、`CLAUDE.md` を足す。`translate/names.rs`、`optimize/handlers/passing.rs`、`touch.rs`、`eml_vm` の `error.rs`、`hosts.rs`、`op.rs` を、各項に役目の1句と一緒に足す (役目は各ファイルの冒頭の doc コメントから取る)。architecture.md:300 の `simplify` の文は [H] だけで、S4 の項目にしない | なし | いいえ |
| a7-8 testing.md の数と列挙 | 事実 [V] `expected.txt` は `#` を除いて 9123 行、`trusted::` は 63 件、types の scaling のテストは 13 本、`eml_vm/tests/main.rs` は `conversions`、`machine`、`reuse`、`roots`、`structs` も宣言する | ほかの文書 | (1) 「期待値の表」の「今は 9103 の場合がある」を消す。(2) 「Miri」の「Apple M4 では、59 個のテストにどちらも約 152 秒かかる (S9b-3 の終わり)」を消す ([H] と同じ文。数と時間と段の名前を一緒に消し、同じ節の「数分で終わる大きさにする」に任せる)。(3) 「性能のテスト」の形の列挙に、`a_ring_of_higher_order_functions`、`a_ring_of_functions_with_local_lambdas`、`many_functions_over_a_long_arrow_chain` の3つを、`a_list_literal_of_calls` と同じくテストの名前を添えて足し、「9つ」の数を書かない。(4) :53 の `eml_test_support` の関数に `run_stats_with_args`、`std_module`、`full_with_paths`、`located` を足す。(5) 「`eml_vm` のテスト」に5ファイルの項を足す (中身は各ファイルの冒頭の doc コメントから取る) | なし | いいえ |
| a7-9 testing.md「分類」の `run/` と `run-fail/` | 事実 [V] `run/` は22、`run-fail/` は9の分類 | ほかの文書 | 一覧に `arrays`、`classes`、`float`、`io`、`kinds`、`linearity`、`lists`、`panic`、`records`、`reuse`、`strings`、`unwind`、`var` の13項を、既存の項と同じ `  - \`名前/\`: 説明` の形で足す。説明は各ディレクトリのテストの名前から1句で書く。「両方で同じ名前を使う」は、`run-fail/` の分類がどれも `run/` の名前なので残す。`scripts/` の項の「S12d の」は消す ([H] と同じ文) | なし | いいえ |
| a7-10 testing.md「分類」の `check-fail/` の規則 | 事実 [V] 番号の範囲の6分類のほかに、機能の分類 `arrays` (2本)、`classes` (25本)、`float` (2本)、`kinds` (19本)、`panic` (4本)、`std` (1本)、`var` (15本) がある | 既定はほかの文書 | 既定では規則を「番号の範囲で分けるのを基本にし、1つの機能に閉じた診断は `run/` と同じ機能の名前の分類に置く」にし、7つの機能の分類を列挙する。もう一方の案は、68本を番号の範囲の分類へ移すこと | 既定ではなし。移す案なら期待値の変更 (範囲は `tests/ui/check-fail/{arrays,classes,float,kinds,panic,std,var}/` の全部。スナップショットの名前が変わる。成否は変わらない) | はい (Q3) |
| a7-11 multicore.md の S10 を未来形で書く文と、冒頭の備えの列挙 ([H] と同じ文) | 事実 | ほかの文書 | :5 の冒頭の段落を今の状態で書く。「マルチコア対応は実装していない。VM はシングルスレッドで、後から並列にできる制約を守る」とし、今の実装の備えに、row の Kind の束の `Never`、持ち越し規則の `σ ≤ Once` のほかに、`eml_rt` の共有の印方式の RC (`share`、負の RC、共有した物体を reuse しない) と所有者の検査、`CodeBase<M>: Sync` と `Task: Send` の型の assert を足し、同じ文書の該当の節を引く。:34 と :262-264 の「S10 では…出し」「並列化を S10 で行わないのは」も、同じく今の状態 (「VM はシングルスレッドで出している」「VM で並列化しないのは」) で書く | なし | いいえ |
| a7-12 multicore.md:271「ネイティブ化は、S14 と、その後の言語の項目より後にある」([H] と同じ文) | 事実 (S14 は終えている) | ほかの文書 | 「ネイティブ化は、ロードマップで言語の項目より後に置いてある」の内容にし、段の名前を書かない。roadmap の見出しは S2 で変わりうるので、見出しの引用は付けない | なし | いいえ |
| a7-13 future/stdlib.md:15「S14 で `Num` が入る」([H] と同じ文) | 事実 [V] `std/Prelude.em` に `class Num`、`class Div` | ほかの文書 | 型クラスの列挙を `Eq`、`Ord`、`Show`、`Num`、`Div` にし、「S14 で…」を消す。同じ箇条の「今は」は、範囲を述べる文なので残してよい | なし | いいえ |
| a7-14 verification.md の行数 | 事実 [V] 数え方によって値が変わる (下の 7) | ほかの文書 | 3か所の行数 (「約 1,650 行」「約 1,200 行」「約 14,000 行」「約 1,600 行」) を消し、数を使わない比べ方で書く。`kind/` は「`eml_types` の中で小さく閉じた部分で、検証する計算は `mod.rs` (大半が由来の型) を除く `entail.rs`、`eval.rs`、`problem.rs`、`solve.rs`、`summary.rs` にある」、型検査の全体は「`kind/` の何倍も大きく」、EP は「`kind/` と同じくらいの大きさだが、Core IR を大きく書き換え、`eml_core_ir` のほかの部分と型を共有する」の内容にする。冒頭の日付は S1 の [H] の作業が扱う | なし | いいえ |
| a7-15 verification.md の S10 を未来形で書く文 ([H] と同じ文) | 事実 | ほかの文書 | :13「S10 の後に、LIR の verifier と VM の側で扱う」を「LIR の verifier と VM の側で扱う」、:17「S10 の VM は、…義務を負う」を「VM は、…義務を負う」、:94「S10 で、UI テストを…検査モードで走らせる。これはすでに決めてある」を「UI テストは検査モードと速いモードで流す ([テスト戦略](../implementation/testing.md) の「2つのモードで流す」)」、:95「S10 の VM の1歩は、後で Verus で書ける大きさに保つ」を「VM の1歩は、Verus で書ける大きさに保つ」、:100「S10 とは独立に進められる」を消し、:101 の手順「S10 で、UI テストを検査モードで走らせる」は済んだ手順なので手順の列から消して番号を詰める | なし | いいえ |
| a7-16 implementation/diagnostics.md のほかの行 | 一致する | 直さない | | | |

#### 足す語の表 (a7-2)

| 行 | 足す語 |
|---|---|
| spec/expressions.md | ローカル関数、リスト、レコード、補間、`var`、並行処理、型の明示 |
| spec/modules.md | REPL のセッション |
| spec/stdlib.md | `Prelude`、`try_io`、`try_panic`、`exit`、`for` と `while`、`Std.Float`、`Std.Array`、`Std.MutArray` |
| spec/types.md | 数の型、フィールドの解決、`var` の変数、REPL の入力 |
| spec/linearity.md | 暗黙の後始末、レコードとタプル、配列の変数 |
| spec/effects.md | 組み込みの `Panic`、`var` のエフェクト、健全性 |
| spec/object-model.md | 配列、ダミーの値、`unsafe` の核 |
| spec/vm.md | 巻き戻し、extern の中断 |
| spec/diagnostics.md | 他の言語の書き方へのヒント |
| implementation/architecture.md | リポジトリ、ソースファイルと位置、外部 crate |
| implementation/testing.md | REPL のトランスクリプト、CLI のテスト、性能のテスト、feature の組み合わせ、よく使うコマンド |

### S14 の完了の確かめの項目 (`conversions`)

| 項目 | 事実か | 直す側 | 直しの中身 | テストの変更 | 聞くか |
|---|---|---|---|---|---|
| s14-1 vm.md、lir.md と bench.rs の `conversions` | 一致する [V] bench.rs のスナップショットの `conversions` は 39 か所すべて 0 (ep_float も)。vm.md は数え方だけを定め、プログラムごとの値を書かない。lir.md に記述はない | 直さない | 古いのは docs/superpowers の設計文書 | なし | いいえ |
| s14-2 `crates/eml_vm/tests/bench.rs` の `EP_FLOAT_CONVERSIONS` と `typed_benches_do_not_convert` | 事実 [V] 定数は 0 で、分岐は両方とも 0 を返す。doc コメントは今の値と合わない | テストのコード | 定数と分岐を消し、どのプログラムでも `assert_eq!(stats.conversions, 0, …)` にする。doc コメントを「どのプログラムも `apply` が実行時に変換しない」の内容にする。コメントが引く benchmarks.md の「S14e-1」の見出しには触らない (S3 が付け替える) | 機械的 | はい (Q4) |

## 2. 根拠

- a1-2: 単体テストがあり、行の途中のタブはレイアウトの列に影響しない。単独の `\r` は、レイアウトが改行と数えないことを確かめた (7)。spec を実装に合わせる。表示 (ariadne) との差は Q1 で聞く。
- a1-6: `{` を外すと `crates/eml_syntax/tests/records.rs` の `anonymous_records_are_syntax_errors_and_are_skipped` (`f { a } = a` が `E0011 1:3 expected a pattern`) が落ちる [V]。`{` を数えるのは grammar.md の既存の規則のためで、意図した振る舞いである。
- a2-10: 意図した制限ではないが、直すのは文法を広げる作業で、spec (grammar.md) と実装は今は一致している。食い違いを直すこの作業では扱わず、既知の制限として残す。
- a3-9: 名前の宣言は extern で、型の正体は要素のないタプルである。両方を正しく書けば足りる。
- a4-1: spec の `type_repr` は内部の関数を正しく指す。食い違いは公開の別名だけにある。
- a5-9: lir.md の R8 の補足がすでに「タグの `switch` の scrutinee は変数」と書く。直すのは architecture.md の言い方だけ。
- a6-7: `Pure` の行は巻き戻しの地点を持たない。届けないのは設計の帰結で、spec とコメントの言い回しを直す。
- a7-8、a7-14: S1 の「数値は benchmarks.md だけが持つ」に合わせ、数を書き直さずに消す。行数は数え方で値が変わり (7)、書き直しても古くなる。
- s14-2: 値は変わらないが、「ep_float だけは別」と読める形は誤った仕様の記録として残る。

## 3. 検討して退けた案

- a1-6: `at_apat_start_at` から `{` を外す。上のテストが落ち、無名のレコードのパターンの E0011 の位置と文言が変わる。
- a1-2: 行の途中のタブをエラーにする。レイアウトに影響せず、意図したテストもある。
- a2-1: repl.md の例を、REPL に標準入力で流した出力と比べて確かめる。標準入力では入力の行が出力に出ないので、例の形 (`> 入力` と出力が交互) と比べられない。トランスクリプトのスナップショットの行で組めば、行ごとに機械的に確かめられる。
- a2-1: 例のために新しいトランスクリプトを足す。displayer.repl が同じ内容を持つので、テストの追加は要らない。
- a2-11: 連鎖の E1002 をこの作業で直す。診断の改善の作業で、spec との食い違いではない。
- a3-4: CLAUDE.md を直さずに残す。Architecture の不変条件の欄にあり、事実の誤りが作業の規則として読まれる。
- a5-9: lir.md に規則の文を足す。同じ事実が R8 の補足にすでにあり、2か所になる。
- a6-10: 83 個の表から配列を外す。表は「83 個」と数を言うので、行をそろえるほうが短い。
- a6-13: effects.md と stdlib.md の両方に完全な列挙を置く。同じ事実が2か所になる。
- a7-8、a7-14: 数を今の値に書き直す。S1 の決まりに反し、変更のたびに古くなる。
- [H] を含む文を S4 で触らず、すべて S1 の [H] の作業に回す。食い違いの直し (古い参照、未来形のままの過去) は同じ文の中にあり、2回に分けると同じ文を2度直す。

## 4. 置いた仮定

- S1 の「決まりの文書化」の単位が、S4 のどの作業より先に入る (0)。S1 の細部が改訂で変わっても、「段の名前と日付を書かず、今の状態を書く」「数値は benchmarks.md だけ」は変わらない [I]。
- S2、S3 は status.md、roadmap.md、benchmarks.md の中身と見出しを扱う。S4 はそれらの見出しを新しく引かない (a7-12 も引用を付けない)。
- この作業で見出しは変えない。`citations.rs` の検査の行き先は変わらない。
- 文書の日本語は、実装者が `yomiyasu:yomiyasu` を呼んでから書く。

## 5. 実装への含意 (コミットの列)

### 共通の確かめ

どのコミットでも最後に流す。

```sh
cargo test                                   # 失敗 0
cargo clippy --all-targets -- -D warnings    # 警告 0
cargo fmt --check                            # 差分なし
cargo test -p eml_cli --test integration citations   # 3 passed
```

文書だけのコミットは、citations が 3 passed で、`git diff --stat` が `docs/` (と W-D5 の CLAUDE.md) だけなら、`cargo test` 全体は省いてよい。各作業の「確かめ」の検索は、すべて直す前に流して「前」の値になることを見てから直し、直した後に「後」の値になることを見る。値は 0fff343a で測った。各コミットのメッセージに、テストの変更の種類を書く。

### 作業の列

すべての作業は S1 の「決まりの文書化」のコミットの後に置く。その中の順は W-C1 → W-C2 → W-D1 → … → W-D17 とする。依存を書いていない作業は、互いに入れ替えてよい。

#### W-C1: `boxed_repr` の公開名 (コード)

- 項目: a4-1、a4-2
- 対象: `crates/eml_core_ir/src/translate/mod.rs`、`crates/eml_core_ir/src/lib.rs`、`crates/eml_core_ir/tests/externs.rs`、`docs/implementation/architecture.md` (「`eml_core_ir`、`eml_lir`、`eml_vm` の内部」の Repr の項)、`docs/implementation/testing.md` (:58)
- テストの変更: 機械的。名前の変更なので、先に書くテストはない。`externs.rs` のテストが変更の前後で同じ値を確かめる
- 確かめ:
  - `grep -c "as type_repr" crates/eml_core_ir/src/translate/mod.rs`: 前 1、後 0
  - `grep -c "type_repr" crates/eml_core_ir/tests/externs.rs`: 前 4、後 0
  - `grep -rc "eml_core_ir::type_repr" docs/implementation | grep -v ':0'`: 前 2行、後 0行
  - `cargo test -p eml_core_ir --test integration externs` が通る
- 依存: なし

#### W-C2: bench.rs の `conversions` の検査を1つにする (テスト)

- 項目: s14-2。Q4 で承認されたときだけ作る
- 対象: `crates/eml_vm/tests/bench.rs`
- テストの変更: 機械的
- 確かめ: `grep -c EP_FLOAT_CONVERSIONS crates/eml_vm/tests/bench.rs`: 前 2、後 0。`cargo test -p eml_vm --test integration bench::` が 51 件通る
- 依存: なし

#### W-D1: 字句の spec

- 項目: a1-1、a1-2、a1-4、a1-5
- 対象: `docs/spec/lexical.md`
- 確かめ:
  - `grep '区切り記号は' docs/spec/lexical.md | grep -c '\\'`: 前 1、後 0
  - `grep -c 'スペースと改行 (' docs/spec/lexical.md`: 前 1、後 0
  - `grep -c 'E0005' docs/spec/lexical.md`: 前 1、後 2
  - `grep -c '1abc' docs/spec/lexical.md`: 前 0、後 1 以上
- 依存: なし

#### W-D2: 文法、レイアウト、式、例の spec と future/stdlib.md の冒頭

- 項目: a1-6、a1-8、a1-9、a1-10、a1-13
- 対象: `docs/spec/grammar.md`、`docs/spec/layout.md`、`docs/spec/expressions.md`、`docs/spec/examples.md`、`docs/future/stdlib.md` (冒頭)
- 確かめ:
  - `grep '関数の形かどうか' docs/spec/grammar.md | grep -c '浮動小数'`: 前 0、後 1
  - `sed -n '/^## 文脈のスタック/,/^## 規則を当てる順/p' docs/spec/layout.md | grep -c E0011`: 前 0、後 2
  - `grep -c E2013 docs/spec/expressions.md`: 前 0、後 1
  - `grep -c 'まだ設計していない\|議論しながら\|S12' docs/spec/examples.md`: 前 1 (同じ行)、後 0
  - `grep -c '別の spec で扱う' docs/future/stdlib.md`: 前 1、後 0
- 依存: なし

#### W-D3: REPL の spec

- 項目: a2-1〜a2-4
- 対象: `docs/spec/repl.md`
- 手順: a2-1 の行をスナップショットから写す
- 確かめ:
  - 例のブロックの各行がスナップショットにあること。`awk '/^### 値の書き方/,/^表示器は/' docs/spec/repl.md | sed -n '/^```$/,/^```$/p' | grep -v '^```' | while IFS= read -r l; do grep -qxF -- "$l" crates/eml_cli/tests/snapshots/integration__repl__transcripts@basics__displayer.repl.snap || echo "MISSING: $l"; done`: 前は `MISSING` が11行 (`> fn x -> x + 1` など)、後は0行
  - `grep -c 'Exited' docs/spec/repl.md`: 前 0、後 1 以上
  - `grep -c '上の「defaulting しない」' docs/spec/repl.md`: 前 1、後 0
- 依存: なし

#### W-D4: モジュールと宣言の spec

- 項目: a2-5〜a2-8、a2-12、a2-13、a2-15
- 対象: `docs/spec/modules.md`、`docs/spec/declarations.md`、`docs/implementation/diagnostics.md` (E1031 の行)
- 確かめ:
  - `grep -c '下の「`import Std.X as F` で' docs/spec/modules.md`: 前 1、後 0
  - `grep -c 'future/stdlib.md' docs/spec/modules.md`: 前 1、後 0
  - `grep -c 'or `import Std' docs/spec/modules.md`: 前 0、後 1 以上
  - `grep -c 'E1002' docs/spec/declarations.md`: 前 0、後 1 以上
  - `grep -c 'E1007' docs/spec/declarations.md`: 前 0、後 1 以上
  - `T() { sed -n '/^Prelude の instance は次のとおり/,/^- `compare`/p' docs/spec/declarations.md; }; for k in 'Num Int' 'Div Int' 'Num Float' 'Div Float' 'Eq Float' 'Ord Float' 'Eq IOErrorKind' 'Show IOErrorKind' 'Show IOError`'; do T | grep -q "$k" || echo "MISSING: $k"; done`: 前は9行、後は0行
  - `grep -c 'まだ Prelude にない' docs/spec/declarations.md`: 前 1、後 0
- 依存: なし

#### W-D5: 型、エフェクト、診断の spec と CLAUDE.md

- 項目: a3-1、a3-2、a3-4 (Q2 で承認されたとき)、a3-5〜a3-10
- 対象: `docs/spec/effects.md` (:79)、`docs/spec/diagnostics.md`、`docs/spec/types.md`、`docs/implementation/diagnostics.md`、`CLAUDE.md` (Architecture の節の1文)、`crates/eml_syntax/src/lib.rs` (コメント)
- 確かめ:
  - `grep -c 'RunConfig::file_root' docs/spec/effects.md`: 前 1、後 0
  - `grep -c '型検査のどの段階でも同じ意味で使う' docs/spec/diagnostics.md`: 前 1、後 0
  - `grep -c 'used by every stage' CLAUDE.md`: 前 1、後 0 (Q2 が承認のとき)
  - `grep -c 'どの段階でも使うので' crates/eml_syntax/src/lib.rs`: 前 1、後 0
  - `D=docs/implementation/diagnostics.md; grep '^| E1017' $D | grep -c 'ローカル関数の引数'; grep '^| E4003' $D | grep -c 'ローカル関数の引数'; grep '^| E1022' $D | grep -c 'この入力'`: 前 0 0 0、後 1 1 1
  - `grep -c '`Int`、`Float`、`String`、`Unit` は `Unr`' docs/spec/types.md`: 前 1、後 0
  - `grep 'row のラベルは `IO` と `Panic` だけ' docs/spec/types.md | grep -c '重ね'`: 前 0、後 1
- 依存: なし

#### W-D6: Core IR の spec

- 項目: a4-3〜a4-7
- 対象: `docs/spec/core-ir.md`、`crates/eml_core_ir/src/translate/program.rs` (`core_effects` の doc コメント)
- 確かめ:
  - `grep -c shows_minus docs/spec/core-ir.md`: 前 0、後 1 以上
  - `grep -c '<extern>' docs/spec/core-ir.md`: 前 1、後 2 以上
  - `grep -c '番号は 0 と 1' docs/spec/core-ir.md`: 前 0、後 1
  - `grep -c '上の「RC の対象でない値の印は' docs/spec/core-ir.md`: 前 1、後 0
  - `sed -n 5p docs/spec/core-ir.md | grep -c '構造体の形'`: 前 0、後 1
- 依存: W-C1 (core-ir.md の `boxed_repr` の説明を W-C1 の名前と読み比べるため)

#### W-D7: LIR の spec と testing.md の LIR の節、architecture.md の `switch`

- 項目: a5-1〜a5-9
- 対象: `docs/spec/lir.md`、`docs/implementation/testing.md` (「LIR のテキストの形」と :51)、`docs/implementation/architecture.md` (:276)
- 確かめ:
  - `grep '値 (アトム) は' docs/spec/lir.md | grep -c Float`: 前 0、後 1
  - `grep -c '分解のときの .release. だけになる' docs/spec/lir.md`: 前 1、後 0
  - `grep -c '出口の生存集合' docs/spec/lir.md`: 前 2、後 0
  - `grep -c 'Perceus はそこで .File. を .dup. する' docs/spec/lir.md`: 前 1、後 0
  - `grep 'Perceus が足すのは\|LIR のパスが足す変数は' docs/spec/lir.md | grep -c token`: 前 0、後 1
  - `grep 'リークの表示' docs/spec/lir.md | grep -c 'vm.md'`: 前 0、後 1
  - `sed -n '/^## LIR のテキストの形/,/^## Core IR のテキストの形/p' docs/implementation/testing.md | grep -c token`: 前 0、後 3 以上
  - `grep -c '出力を所有の段' docs/implementation/testing.md`: 前 0、後 1
  - `grep -c 'verifier が定数で分ける .switch. を拒む' docs/implementation/architecture.md`: 前 1、後 0
- 依存: W-C1 (testing.md と architecture.md の同じファイル)

#### W-D8: VM の spec と `failed` のコメント

- 項目: a6-1〜a6-8 (a6-7 は vm.md とコメント)
- 対象: `docs/spec/vm.md`、`crates/eml_vm/src/vm.rs` (`failed` の doc コメント)
- 確かめ:
  - `grep -c '.unsafe. の呼び出しは、この関数の中だけ' docs/spec/vm.md`: 前 1、後 0。`grep -c stdout_width docs/spec/vm.md`: 前 0、後 1
  - `grep 'から再公開し' docs/spec/vm.md | grep -c Finished`: 前 0、後 1
  - `grep '実行の状態は、ワーカーごとの' docs/spec/vm.md | grep -o 'reuses\|conversions\|stack_limit\|keep_result\|`entry`\|`exit`' | sort -u | wc -l`: 前 0、後 6
  - `grep -c compile_error docs/spec/vm.md`: 前 0、後 1 以上
  - `grep '`CompileError` は、スロットが' docs/spec/vm.md | grep -c NoConversion`: 前 0、後 1
  - `grep -c 'runtime error: ' docs/spec/vm.md`: 前 0、後 1 以上
  - `grep -rn 'purity. によらず' docs/spec/vm.md crates | wc -l`: 前 2、後 0
- 依存: なし

#### W-D9: オブジェクトモデルの spec

- 項目: a6-9〜a6-11
- 対象: `docs/spec/object-model.md`
- 確かめ:
  - `sed -n '/^### 誤り$/,/^### 誤りのときの約束/p' docs/spec/object-model.md | grep -c 'FloatToInt\|FixedDigits'`: 前 0、後 2 以上
  - `sed -n '/^## extern$/,/^### 語の ABI/p' docs/spec/object-model.md | grep -c array_make`: 前 0、後 1
  - `grep 'unit. の引数を持つ行' docs/spec/object-model.md | grep -c 'MutArray.empty'`: 前 0、後 1
- 依存: なし

#### W-D10: 標準ライブラリとエフェクトの `Panic` の一覧

- 項目: a6-7 (stdlib.md)、a6-12、a6-13
- 対象: `docs/spec/stdlib.md`、`docs/spec/effects.md` (「組み込みの `Panic`」)
- 確かめ:
  - `sed -n '/^| `Fault` | 起こす関数 |/,/^## /p' docs/spec/stdlib.md | grep -o 'Float\.truncate\|Float\.floor\|Float\.ceil\|Float\.round\|Float\.to_fixed\|MutArray\.read\|MutArray\.write\|MutArray\.make\|Array\.get\|Array\.set\|Array\.make\|check_index' | sort -u | wc -l`: 前 0、後 12
  - `grep -c 'purity. によらず' docs/spec/stdlib.md`: 前 1、後 0
  - `grep 'これらを起こしうる関数は\|一覧は' docs/spec/effects.md | grep -c 'stdlib.md'`: 前 0、後 1
- 依存: なし。W-D8 と合わせて、`grep -rn 'purity. によらず' docs crates` が 0 件になる

#### W-D11: testing.md の数と列挙

- 項目: a7-8
- 対象: `docs/implementation/testing.md` (「期待値の表」「Miri」「性能のテスト」「`eml_vm` のテスト」と :53。S1 が持つ節は触らない)
- 確かめ:
  - `grep -c '9103\|59 個' docs/implementation/testing.md`: 前 2、後 0
  - `grep -c 'の9つである' docs/implementation/testing.md`: 前 1、後 0
  - `for w in a_ring_of_higher_order_functions a_ring_of_functions_with_local_lambdas many_functions_over_a_long_arrow_chain run_stats_with_args std_module full_with_paths located; do grep -q "\`$w\`" docs/implementation/testing.md || echo "MISSING: $w"; done`: 前は7行、後は0行
  - `sed -n '/^## `eml_vm` のテスト/,/^## LIR のテキストの形/p' docs/implementation/testing.md | grep -o 'conversions.rs\|structs.rs\|reuse.rs\|machine.rs\|roots.rs' | sort -u | wc -l`: 前 0、後 5
- 依存: W-D7 (同じファイル)

#### W-D12: testing.md の UI テストの分類

- 項目: a7-9、a7-10 (Q3 の答えに従う)
- 対象: `docs/implementation/testing.md` (「分類」)。Q3 で移す案になれば、`tests/ui/check-fail/` の7分類とそのスナップショットも
- 手順 (移す案のとき): 各テストを主な番号の範囲の分類へ `git mv` する。`cargo insta test -p eml_cli --test integration --review` でスナップショットを受け入れ、`cargo insta test -p eml_cli --test integration --unreferenced=delete` で古いものを消す
- テストの変更: 既定ではなし。移す案なら期待値の変更 (範囲は a7-10)
- 確かめ:
  - `for c in arrays classes float io kinds linearity lists panic records reuse strings unwind var; do sed -n '/^### 分類/,/^### 2つのモードで流す/p' docs/implementation/testing.md | grep -q "^  - \`$c/\`" || echo "MISSING: $c"; done`: 前は13行、後は0行
  - `grep -c 'S12d' docs/implementation/testing.md`: 前 1、後 0
  - 既定のとき: `for c in arrays classes float kinds panic std var; do sed -n '/^- `check-fail\/`/,/^- サブディレクトリの名前/p' docs/implementation/testing.md | grep -q "\`$c/\`" || echo "MISSING: $c"; done`: 前は7行、後は0行
  - 移す案のとき: `ls tests/ui/check-fail`: 後は `exhaustiveness linearity names not-yet-supported syntax types` の6つだけ。`cargo test -p eml_cli --test integration ui::` が通り、`git status --short crates/eml_cli/tests/snapshots` に未追跡のファイルがない
- 依存: W-D11 (同じファイル)

#### W-D13: README の地図の「内容」

- 項目: a7-2 (a2-14 を含む)
- 対象: `docs/README.md` (足す語の表の11行だけ)
- 確かめ: 足す語の表の各語が、その行に出ること。例 `grep '^| \[spec/stdlib.md\]' docs/README.md | grep -o 'Prelude\|try_io\|try_panic\|`exit`\|`while`\|Std.Float\|Std.Array\|Std.MutArray' | sort -u | wc -l`: 前 0、後 8。ほかの10行も同じ形で、表の語の数になる (前は 0。spec/effects.md の行の `Panic` のように既存の語と重ならない語で数える)
- 依存: なし

#### W-D14: overview の設計判断の表

- 項目: a7-5
- 対象: `docs/overview.md` (「確定した設計判断」)
- 確かめ: `for k in Panic '`var`' Num MutArray; do grep -q "$k" docs/overview.md || echo "MISSING: $k"; done`: 前は4行、後は0行
- 依存: なし

#### W-D15: architecture.md の CLI と構成

- 項目: a7-6、a7-7
- 対象: `docs/implementation/architecture.md` (「リポジトリ」「`eml_core_ir`、`eml_lir`、`eml_vm` の内部」「CLI と lib API」)
- 確かめ:
  - 木のコードブロックの中で: `sed -n '/^eml\/$/,/^```$/p' docs/implementation/architecture.md | grep -c 'bench/\|Cargo.lock\|CLAUDE.md'`: 前 0、後 3
  - `for w in names.rs passing.rs touch.rs hosts.rs op.rs error.rs script_host split_script_args; do grep -q "$w" docs/implementation/architecture.md || echo "MISSING: $w"; done`: 前は8行、後は0行
- 依存: W-C1、W-D7 (同じファイル)

#### W-D16: multicore.md

- 項目: a7-11、a7-12
- 対象: `docs/future/multicore.md`
- 確かめ:
  - :5、:34、:262-264 の行の S10: `sed -n '5p;34p;262,264p' docs/future/multicore.md | grep -c 'S10'`: 前 4、後 0 (行番号は直す前のもの。直した後に行がずれたら、同じ段落を `grep -n` で探して見る)
  - `grep -c 'S14 と、その後の言語の項目より後' docs/future/multicore.md`: 前 1、後 0
  - `sed -n 5p docs/future/multicore.md | grep -c 'share'`: 前 0、後 1 (冒頭の段落が1行のままのとき)
- 依存: なし

#### W-D17: future/stdlib.md と verification.md

- 項目: a7-13、a7-14、a7-15
- 対象: `docs/future/stdlib.md`、`docs/future/verification.md`
- 確かめ:
  - `grep -c 'S14 で .Num. が入る' docs/future/stdlib.md`: 前 1、後 0
  - `grep -c '1,650\|14,000\|1,600\|1,200' docs/future/verification.md`: 前 3、後 0
  - `grep -c 'S10' docs/future/verification.md`: 前 6、後 0
- 依存: W-D2 (future/stdlib.md の同じファイル)

## 6. 未決の事項

- Q1 (a1-2): 単独の `\r` はレイアウトで改行にならない (1行として読む) が、診断の表示 (ariadne) はそこで行を分けて見せる。このまま「空白だが改行ではない」と spec に書いてよいか。既定: そう書く。表示との差は今は直さない。エラーにする、または改行として数えるなら、lexer かレイアウトを変える別の作業になる。
- Q2 (a3-4): CLAUDE.md の Architecture の E0004 の理由の文を "means the same at every stage" に直してよいか。規則でなく理由の事実の誤りを直すだけである。既定: 直す。
- Q3 (a7-10): `check-fail/` の機能の分類にある68本を番号の範囲の分類へ移すか、規則を「番号の範囲を基本にし、1つの機能に閉じた診断は機能の分類」に改めるか。既定: 規則を改める (テストは動かさない)。
- Q4 (s14-2): bench.rs の `EP_FLOAT_CONVERSIONS` と分岐を消してよいか。期待する値は変わらないが、利用者の全体の規則ではテストの整理に承認が要る。既定: この計画の承認をもって進める。承認しないなら W-C2 を作らない。

## 7. 追加の確認

- [V] E0004 を出すのは `eml_hir` だけ (`grep -rn not_yet_supported crates/*/src`)。
- [V] REPL: シグネチャのない `inc x = x + 1` は E1004。displayer.repl のスナップショットが a2-1 で使う行をすべて持つ。
- [V] a1-6: リポジトリを `git archive HEAD` で作業ディレクトリ (`S4-scratch/repo`) に写し、`{` を外して `cargo test -p eml_syntax` (1件失敗) と `cargo test -p eml_cli --test integration` (177 passed) を流した。リポジトリは書き換えていない。
- [V] 単独の `\r`: `printf 'x : Int\rx = 1\n'` のファイルは E1005 と E1015 (`Int` に引数1つ) になり、1行として読まれる。ariadne の表示はこの入力を2行に分けて見せる。
- [V] E0007 (`0XFF`、`1abc`)、E0005、E1002 (`deriving (Foo)`)、E1007 (`op : Int`)、`main` の row の重複、E1031 の2つの help を `eml check|run` で確かめた (ファイルは `S4-scratch/`)。
- [V] エフェクトの番号: `translate/program.rs` の `core_effects` が `hir.effects()` の順に並べ、`effect_index` がその位置を番号にする。Prelude は `IO` (:37)、`Panic` (:50) の順に宣言する。
- [V] lir.md の R8 の補足に「タグの `switch` の scrutinee は変数」がある (lir.md:83)。`check.rs:712-717` がそれを確かめる。
- [V] `purity` の言い回しは vm.md:315、stdlib.md:101、`crates/eml_vm/src/vm.rs:999` の3か所にある。
- [V] `kind/` の行数は数え方で変わる。各ファイルの最初の `#[cfg(test)]` で切ると 1,772 行だが、`solve.rs` の 692 行目の `#[cfg(test)]` は `residual_of` の1関数だけにかかり、テストのモジュールは 801 行目から始まる。その関数だけを除くと約 1,870 行になる。数を書かない形にしたのはこのため。
- [V] 確かめの「前」の値は、すべて 0fff343a で上の検索を流して測った。


---

## W-D12 (置き換え)


付録 D (d-spec-fixes.md) の W-D12 を、この文書で置き換える。ユーザーの決めにより、r2 の既定 (testing.md の規則を改めて機能の分類を認める) を取り消し、`tests/ui/check-fail/` の機能の分類にある68本を、番号の範囲の分類へ移す。行番号は 0fff343a のものである。

## 1. 範囲と種類

- 項目: a7-9 (run/ と run-fail/ の分類の一覧)、a7-10 (check-fail/ の分類)
- テストの変更の種類: 期待値の変更 (UI テストの移動で、成否は変わらない)。work の spec には次の範囲で書く: 「`tests/ui/check-fail/` の `arrays/`、`classes/`、`float/`、`kinds/`、`panic/`、`std/`、`var/` の68本を、主な診断の番号の範囲の分類 (`syntax/`、`names/`、`types/`、`linearity/`、`not-yet-supported/`) へ移す。スナップショットは、名前と、中のテストのパスの行だけが変わる」。理由は「testing.md の「分類」は check-fail/ を番号の範囲で分けると決めており、機能の分類はその規則に反するため」
- 移さないもの: `run/` と `run-fail/` の分類 (機能で分ける規則のとおりである)。`check-fail/` の `syntax/`、`names/`、`types/`、`linearity/`、`exhaustiveness/`、`not-yet-supported/` にある115本
- 移す先の本数: `types/` 25、`linearity/` 20、`names/` 18、`syntax/` 3、`not-yet-supported/` 2 (合わせて68)。`exhaustiveness/` へ移すものはない

## 2. 移し先の決め方

testing.md の規則 (「分類」の `check-fail/` の箇条) に従い、主な診断の番号で決める。E0004 は `not-yet-supported/`、ほかの E0xxx は `syntax/`、E1xxx は `names/`、E2xxx は `types/`、E3xxx は `linearity/` (E3001 も含む)、E4xxx は `exhaustiveness/` である。

主な診断は次の順で決める。

1. テストのファイル (ディレクトリのテストでは `main.em`) の先頭のコメント (`--` で始まる行の続き) に番号があれば、最初に書いた番号
2. コメントに番号がなければ、スナップショットに出る番号。68本のうちコメントに番号のない3本 (`arrays/lin_element_get.em`、`arrays/mut_array_twice.em`、`std/to_option_linear_error.em`) は、どれもスナップショットに1つの範囲の番号しか出ない

[V] 68本のスナップショットの番号を読んだ。2つ以上の範囲の番号を出すのは次の2本だけで、どちらもコメントで決まる。

- `classes/not_derivable.em`: E1038 と E2006 を出す。コメントは「E1038: only Eq, Ord and Show can be derived. E2006: …」で、最初は E1038 なので `names/`
- `kinds/extern_kind.em`: E1033 と E0011 を出す。コメントは「E0011: extern の型の Kind は…」だけなので `syntax/`。E1033 は、ユーザーのモジュールが `extern` を書いたことに付いて出る

ほかの66本は、コメントの番号とスナップショットの番号が同じ範囲にある。`classes/extern_in_instance.em` はコメントに E1033 と E1036 を書くが、スナップショットは E1033 だけで、どちらも `names/` である。

## 3. ファイルごとの移し先

「今の場所」は `tests/ui/check-fail/` からのパスで、新しい場所は「新しい分類」の下の同じ名前である。

| 今の場所 | 新しい分類 | スナップショットの番号 | 先頭のコメントの番号 |
|---|---|---|---|
| `arrays/lin_element_get.em` | `linearity/` | E3001 | なし |
| `arrays/mut_array_twice.em` | `linearity/` | E3002 | なし |
| `classes/ambiguous_constraint.em` | `types/` | E2009 | E2009 |
| `classes/ambiguous_empty_list.em` | `types/` | E2009 | E2009 |
| `classes/ambiguous_interpolation.em` | `types/` | E2009 | E2009 |
| `classes/ambiguous_num.em` | `types/` | E2009 | E2009 |
| `classes/class_as_type.em` | `names/` | E1043 | E1043 |
| `classes/constrained_polymorphic_recursion.em` | `types/` | E2012 | E2012 |
| `classes/deriving_linear_field.em` | `types/` | E2010 | E2010 |
| `classes/duplicate_instance.em` | `names/` | E1035 ×2 | E1035 |
| `classes/extern_in_instance.em` | `names/` | E1033 | E1033、E1036 |
| `classes/interpolation_without_show.em` | `types/` | E2006 | E2006 |
| `classes/invalid_constraint.em` | `names/` | E1040 ×3 | E1040 |
| `classes/invalid_instance_head.em` | `names/` | E1039 ×3 | E1039 |
| `classes/linear_instance_head.em` | `types/` | E2010 | E2010 |
| `classes/method_kind_mismatch.em` | `types/` | E2011 | E2011 |
| `classes/method_variable_growth.em` | `types/` | E2012 | E2012 |
| `classes/missing_method.em` | `names/` | E1036 | E1036 |
| `classes/missing_superclass_instance.em` | `types/` | E2006 | E2006 |
| `classes/nested_data_type_show.em` | `types/` | E2012 | E2012 |
| `classes/no_instance.em` | `types/` | E2006 ×3 | E2006 |
| `classes/not_a_class.em` | `names/` | E1041 | E1041 |
| `classes/not_derivable.em` | `names/` | E1038、E2006 | E1038、E2006 |
| `classes/orphan_instance/ (main.em、Color.em、Size.em)` | `names/` | E1034 | E1034 |
| `classes/superclass_context_not_entailed.em` | `types/` | E2006 | E2006 |
| `classes/superclass_cycle.em` | `names/` | E1042 | E1042 |
| `classes/unknown_method.em` | `names/` | E1037 | E1037 |
| `float/float_pattern.em` | `names/` | E1052 ×2 | E1052 |
| `float/mixed_int_float.em` | `types/` | E2001 ×2 | E2001 |
| `kinds/argument_sorts.em` | `names/` | E1048 ×3 | E1048 |
| `kinds/duplicate_parameter.em` | `names/` | E1003 | E1003 |
| `kinds/extern_kind.em` | `syntax/` | E0011、E1033 | E0011 |
| `kinds/iter_across_multi.em` | `linearity/` | E3006 | E3006 |
| `kinds/lin_deriving.em` | `types/` | E2010 ×2 | E2010 |
| `kinds/missing_row_argument.em` | `names/` | E1015 | E1015 |
| `kinds/once_called_twice.em` | `linearity/` | E3002 | E3002 |
| `kinds/once_declaration.em` | `names/` | E1050 ×3 | E1050 |
| `kinds/once_discarded.em` | `linearity/` | E3004 | E3004 |
| `kinds/once_not_a_function.em` | `syntax/` | E0011 | E0011 |
| `kinds/parameter_called_twice_in_once_field.em` | `linearity/` | E3002 ×2 | E3002 |
| `kinds/row_parameter_instance.em` | `not-yet-supported/` | E0004 ×2 | E0004 |
| `kinds/row_parameter_not_a_name.em` | `syntax/` | E0011 ×2 | E0011 |
| `kinds/token_discarded.em` | `linearity/` | E3004 | E3004 |
| `kinds/token_used_twice.em` | `linearity/` | E3002 | E3002 |
| `kinds/top_level_function_through_effect_label.em` | `linearity/` | E3002 | E3002 |
| `kinds/unknown_kind.em` | `names/` | E1049 | E1049 |
| `kinds/unr_declaration.em` | `not-yet-supported/` | E0004 | E0004 |
| `kinds/unr_value_in_once_field.em` | `types/` | E2001 | E2001 |
| `panic/arithmetic_needs_panic.em` | `types/` | E2002 | E2002 |
| `panic/main_with_other_effect.em` | `types/` | E2004 | E2004 |
| `panic/show_cannot_panic.em` | `types/` | E2002 | E2002 |
| `panic/value_cannot_panic.em` | `types/` | E2002 | E2002 |
| `std/to_option_linear_error.em` | `linearity/` | E3001 | なし |
| `var/across_multi.em` | `linearity/` | E3006 | E3006 |
| `var/assign_array.em` | `linearity/` | E3008 | E3008 |
| `var/assign_target.em` | `names/` | E1053 | E1053 |
| `var/escape_by_value.em` | `types/` | E2019 | E2019 |
| `var/escape_through_outer_local.em` | `types/` | E2019 | E2019 |
| `var/escape_through_state.em` | `types/` | E2019 | E2019 |
| `var/file_var.em` | `linearity/` | E3007 | E3007 |
| `var/lin_element.em` | `linearity/` | E3001 | E3001 |
| `var/not_an_array_index.em` | `types/` | E2001 | E2001 |
| `var/not_taken.em` | `linearity/` | E3003 | E3003 |
| `var/not_taken_without_last.em` | `linearity/` | E3003 | E3003 |
| `var/taken_before_last.em` | `linearity/` | E3008 | E3008 |
| `var/taken_in_a_lambda.em` | `linearity/` | E3008 | E3008 |
| `var/taken_twice.em` | `linearity/` | E3002 | E3002 |
| `var/used_beside_the_take.em` | `linearity/` | E3008 | E3008 |

[V] 移した後の名前は、移し先にある名前とも、移すもの同士とも重ならない (`tests/ui/check-fail/<新しい分類>/` に同じ名前のファイルもディレクトリもない)。

## 4. 付属のファイルの扱い

- ディレクトリのテストは `classes/orphan_instance/` の1つだけで、`main.em`、`Color.em`、`Size.em` を持つ。ディレクトリごと `names/orphan_instance/` へ移す。中のファイルは変えない (import はディレクトリの中のモジュールを名前で指すので、パスに頼らない)
- `.args`、`.stdin`、`.env` の付属のファイルは、7つの分類のどこにもない [V] `find tests/ui/check-fail/{arrays,classes,float,kinds,panic,std,var} -type f ! -name '*.em'` が何も出さない
- スナップショットは `crates/eml_cli/tests/snapshots/integration__ui__check_fail@<分類>__<名前>.snap` である (`.em` のテストでは `<名前>` が `ambiguous_num.em` のように拡張子を含み、ディレクトリのテストでは `orphan_instance`)。68本それぞれの1つを、新しい分類の名前へ `git mv` する
- スナップショットの中身は、テストのパスを含む行だけが変わる。ヘッダの `input_file: tests/ui/check-fail/<分類>/<名前>` と、ariadne の位置の行 (`╭─[ check-fail/<分類>/<名前>:6:31 ]`、`├─[ … ]`) である。`check-fail/<今の分類>/<名前>` を `check-fail/<新しい分類>/<名前>` に置き換える。ariadne の枠の幅は行番号の桁で決まり、パスの長さで変わらないので、ほかの行は変わらない
- 空になった7つのディレクトリは消す (git は空のディレクトリを持たないが、作業ツリーに残る)
- ほかに引く所はない [V] `grep -rnE '(check-fail|check_fail@)/?(arrays|classes|float|kinds|panic|std|var)(/|__)' --exclude-dir=target --exclude-dir=.git --exclude-dir=snapshots .` の当たりは `docs/superpowers/` の8ファイルだけで、S3 の D が先に消す。`ui.rs` は glob (`check-fail/**/*.em`) で集め、分類の名前を持たない

## 5. 手順

S4-scratch で試した形である。リポジトリの根で流す。`moves.tsv` は3章の表を「今の分類 TAB 名前 TAB 主な番号 TAB 新しい分類」の行にしたもので、名前は `.em` のテストでは `ambiguous_num.em`、ディレクトリのテストでは `orphan_instance` と書く。

```bash
S=crates/eml_cli/tests/snapshots
while IFS=$'\t' read -r from name code to; do
  git mv "tests/ui/check-fail/$from/$name" "tests/ui/check-fail/$to/$name"
  git mv "$S/integration__ui__check_fail@${from}__${name}.snap" "$S/integration__ui__check_fail@${to}__${name}.snap"
  sed -i.bak "s#check-fail/$from/$name#check-fail/$to/$name#g" "$S/integration__ui__check_fail@${to}__${name}.snap"
  rm "$S/integration__ui__check_fail@${to}__${name}.snap.bak"
done < moves.tsv
find tests/ui/check-fail -type d -empty -delete
```

`cargo insta` は要らない。スナップショットを手で写して置き換えるので、テストは新しいスナップショットとそのまま一致する。`cargo insta review` で受け入れる手順を使うと、名前の変わったスナップショットが新しいものとして出て、古いものが参照されずに残るので、手で写すほうが差分が小さく、確かめやすい。

## 6. testing.md の直し

- 「分類」の `run/` と `run-fail/` の一覧に、今ある13の分類 (`arrays`、`classes`、`float`、`io`、`kinds`、`linearity`、`lists`、`panic`、`records`、`reuse`、`strings`、`unwind`、`var`) を足す (a7-9。r2 のとおり)
- `scripts/` の箇条の「S12d の5本のスクリプト」を「5本のスクリプト」にする (r2 のとおり)
- `check-fail/` の箇条は変えない。移した後は、規則に挙げた6つの分類だけになる
- :274 と :275 の語は W-H13 が直す (付録 E (e-history.md))

## 7. 確かめ

- `ls tests/ui/check-fail`: 前は13のディレクトリ、後は `exhaustiveness linearity names not-yet-supported syntax types` の6つだけ
- `for d in syntax names types linearity exhaustiveness not-yet-supported; do echo "$d $(ls tests/ui/check-fail/$d | wc -l)"; done`: 前は 15、38、17、37、7、1、後は 18、56、42、57、7、3
- `ls crates/eml_cli/tests/snapshots | grep -c 'check_fail@'`: 前も後も 183
- `ls crates/eml_cli/tests/snapshots | grep -cE 'check_fail@(arrays|classes|float|kinds|panic|std|var)__'`: 前 68、後 0
- `git add -A` の後の `git diff --cached -M --stat` で、移したファイルが138 (テストの70ファイルとスナップショットの68) で、スナップショットの差分がパスの行だけであること。`git diff --cached -M crates/eml_cli/tests/snapshots | grep '^[-+][^-+]' | grep -vc 'check-fail/'` が 0
- `INSTA_UPDATE=no cargo test -p eml_cli --test integration ui::` が通り、`git status --short --untracked-files=all crates/eml_cli/tests/snapshots` に `.snap.new` がない
- r2 の確かめのうち残すもの: `for c in arrays classes float io kinds linearity lists panic records reuse strings unwind var; do sed -n '/^### 分類/,/^### 2つのモードで流す/p' docs/implementation/testing.md | grep -q "^  - \`$c/\`" || echo "MISSING: $c"; done`: 前は13行、後は0行。`grep -c 'S12d' docs/implementation/testing.md`: 前 1、後 0
- 共通の確かめ (r2 の 5 章)

[V] 0fff343a を S4-scratch/repo-mv に写し、5章の手順を流した。移したファイルは138で、`INSTA_UPDATE=no cargo test -p eml_cli --test integration ui::` は9本とも通り、`.snap.new` は出なかった。

## 8. コミット

2つに分ける。

1. `Move check-fail tests into the code-range categories`: 5章の手順だけ (テストとスナップショット)。メッセージに、期待値の変更であること (成否は変わらず、スナップショットの名前とパスの行だけが変わる) と、理由 (testing.md の「分類」の規則) を書く
2. `List the run test categories in testing.md`: 6章の直し

1つにまとめてもよいが、分けると、1つ目の差分が名前の変更とパスの行だけになり、確かめやすい。

## 9. 依存

- S3 の D (`docs/superpowers/` の削除)。先に入っていないと、4章の grep に当たりが残る (移す動作には影響しない)
- W-D11 (同じ testing.md)。コミット1は testing.md に触れないので、W-D11 によらない
- W-H13 はこの作業の後に置く
