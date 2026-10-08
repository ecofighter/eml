# S3b-2a Core IR v2 の構造 (設計)

位置づけ: 作業の設計。再設計のサブプロジェクト S3b-2 の前半、S3b-2a の spec である。全体の方針は [ロードマップ](../../future/roadmap.md) の「S3b-2 Core IR v2」と、[全体設計](2026-10-07-redesign-design.md) にある。決まったことは S3b-2a の終わりに `docs/` の該当文書へ移し、この文書を削除する。

## 目的

Core IR を、アリーナと式ごとの ID から、前向きの辺だけを持つ基本ブロックの列に作り直す。translate、verifier、生存解析、Perceus、インタプリタをこの形に合わせ、`simplify` を translate の変換と小さな DCE に置き換える。実行時エラーにはソースの位置を付ける。

この形を選んだのは、将来インタプリタをバイトコード VM に組み替えることと、ネイティブ化 (Cranelift か LLVM) の土台にすることを見込んだためである。ブロックの列は、バイトコードへは1回の線形なループで、Cranelift のブロック引数と LLVM の phi へは 1:1 で写る。

## S3b-2 の分け方

ロードマップの S3b-2 を2つの段に分ける。

- S3b-2a (この文書): 構造。基本ブロックの列、テキストの形、verifier と生存解析と Perceus の作り直し、translate の作り直し (決定木、タプルの列、条件と case-of-case の出口、末尾呼び出し)、DCE だけの縮約パス、extern の呼び出しの位置
- S3b-2b: 所有と表現。scrutinee を消費しない `Switch` (と `Unpack`)、Repr の確定 (多相な位置の規則、`Float`) と box/unbox、extern の表の Repr、`TailCall` の降格

分ける理由は、`Switch` の所有と Repr に、ブロックの形と関係のない設計上の論点が多いためである。消費しない `Switch` は、`Lin` の scrutinee、一意な箱での RC の操作の数、呼び出しをまたぐ借用、定数の scrutinee と衝突する。Repr は、型変数の表現、`Fn` の表現、クロージャの呼び出しの規約、Repr が違うときの末尾呼び出しを決める必要がある。S3b-2a は、これらを後で足しても IR のテストを作り直さずに済む形にしておく。

## 背景

- `CoreFn` は式のアリーナ (`exprs`、`CExprId`) と join の索引 (`joins`) を持つ。`compact` が各パスの後でアリーナを詰め直し、生存解析が `Join` に `captures` を書き込み、verifier はその `captures` を宣言として読む
- インタプリタの制御は `(関数, CExprId)` で、`Frame::Return.resume` は `CExprId` を `u32` で持つ
- 値の `if` と `match` の続きは join の本体に入れ子になり、共有された `match` の枝は join の scope に入れ子になる。分解の `let` は、`jump` が1つの join になり、`simplify` の B3 が続きを1つの枝の `Switch` の中へ移す。Perceus と verifier はこの入れ子を再帰でたどり、E0013 はその深さを抑えない。1000 の共有された枝で debug ビルドのスタックがあふれる ([実装の現在地](../../implementation/status.md))
- `simplify` は8つの書き換えを9回適用する (F、B3 を2回、K1、B2、B5、B4、DCE、T)。B2 と K1 は枝の部分木全体を置き換えるので、長い `else if` の連鎖で2乗の時間がかかる。末尾呼び出しは T だけが作る
- 実測では、data の case-of-case (B2) が効くのは、それを確かめる2本の UI テストの中の7か所だけで、ほかの UI テストと `std/` では0回である。一方、`let (lo, hi) = if .. then (a, b) else (b, a)` のような書き方のループでは、落とすとインタプリタで 6〜22% 遅くなる
- 実行時エラーは関数の名前だけを持つ (``division by zero in `divide` ``)。値として使う extern は共有のラッパー `extern$X` を通るので、`path |> Fs.open` の失敗は `extern$Std.Fs.open` を指す
- `SourceFiles::line_col` は、呼ばれるたびにファイルの先頭から行を数える

## 決めたこと

### データモデル

```rust
pub struct Program { functions: Vec<CoreFn>, entry: FnIdx, strings: Vec<String>, effects: Vec<EffectInfo>, files: Vec<String> }
pub struct CoreFn  { name: String, vars: Vec<VarInfo>, ret: Repr, blocks: Vec<Block> }
pub struct VarInfo { name: String, repr: Repr }
pub enum   Repr    { Obj, TObj, Int, Enum, Unit }
pub struct BlockId(pub u32);
pub struct Block   { params: Vec<VarId>, stmts: Vec<Stmt>, term: Term }
pub enum   Stmt    { Let { var: VarId, rhs: Rhs }, Unpack { value: VarId, tag: u32, fields: Vec<VarId> }, Dup(VarId), Decref(VarId) }
pub enum   Term    { Return(Atom), TailCall { call: Call, mask: Vec<u32> }, Jump { target: BlockId, args: Vec<Atom> },
                     Switch { scrutinee: Atom, cases: Vec<Case>, default: Option<BlockId> } }
pub struct Case    { pattern: CasePattern, fields: Vec<VarId>, target: BlockId }
pub enum   Rhs     { Call { call, mask, saved }, MakeClosure(..), Extern { ext: Extern, args: Vec<Atom>, at: Option<Loc> },
                     ConstString(u32), Con { tag, args }, Drop(Atom) }
pub struct Loc     { file: u32, line: u32, column: u32 }
```

- `blocks[0]` が入口で、その引数が関数の引数である。`CoreFn.params` はなくなる
- `ret` は、直接の呼び出しの結果の Repr である
- `Loc.file` は `Program.files` (表示用のパス) の添字で、`column` は1から数える文字の位置である
- 削除するもの: `CExprId`、`JoinId`、`CoreFn.joins`、`captures`、`compact.rs`、`builder.rs`、`simplify.rs`、IR に書き込む生存解析、`Rhs::Atom`、translate の `Binding::{Join, Shared}`

Repr は型から決まる関数にする。RC の対象 (`is_rc` は `Obj` と `TObj`) は、今の `boxed` とまったく同じである。

| 型 | Repr |
|---|---|
| `Int` | `int` |
| Unit、空のレコード、`Error` | `unit` |
| 引数のないコンストラクタだけの data | `enum` |
| `String`、`File`、要素のあるタプル、全コンストラクタがフィールドを持つ data | `obj` |
| 引数のないコンストラクタとフィールドを持つコンストラクタが混ざる data | `tobj` |
| 関数 (`Atom::Fn` は即値)、型変数 | `tobj` |

多相な位置 (総称的なフィールド、`apply`、`perform`、`resume`、`handle` の結果) の束縛は、S3b-2a では今と同じく具体化した型から Repr を決める。S3b-2b がその位置の規則を決め、一部の束縛の Repr を変えうる。

### 構造の規則

verifier が検査する。

- R1: 入口のブロックへ向かう辺はない
- R2: 辺はすべて、番号の大きいブロックへ向かう
- R3: `switch` の行き先 (case と default) は引数を持たず、入る辺はその `switch` の辺1本だけである。合流するブロック (`jump` で入るブロック) は、`switch` の行き先にならない
- R4: 入口と `switch` の行き先を除くブロックには、`jump` が1本以上入る。`jump` の実引数の数は、行き先の引数の数と同じである
- R5: 変数は、関数の中で1回だけ定義される。定義する位置は、ブロックの引数、`Let`、`Unpack` のフィールド、`Case` のフィールドである
- R6: 変数が見えるのは、その定義の位置が使う位置を支配するときだけである (同じブロックなら前の文)。所有の検査の段では、合流するブロックに入るすべての `jump` で所有の多重集合が一致しなければならず、その集合が入口の所有になる
- R7: 所有の検査の段では、`let x = call .. save S` の後に見える変数は S と x だけである。S は呼び出しの前に見えていなければならず、S のうち RC の対象の部分は、所有の多重集合と一致する
- R8: `jump` の実引数が変数なら、行き先の引数と Repr が同じである。定数は、行き先の引数の Repr に収まる。`Unpack` の値は Repr が `obj` の変数で、フィールドは1つ以上ある。`return` の値が変数なら、Repr は `ret` と同じである (呼び出しの結果と呼ばれる関数の `ret` は、S3b-2a では比べない)

今の verifier の検査 (`mask` の順、`handle` の節の数、再開できるかどうか、extern の引数の数、`==` を残さないこと、case の種類、リテラルの `switch` の default) は、そのまま引き継ぐ。

意味は次のとおりである。

- `jump` は並列な代入である。実引数をすべて読んでから、行き先の引数に書く
- `Case` のフィールドと `Unpack` のフィールドは、同じ「フィールドの束縛」の規則に従う。S3b-2a では値を消費して作り、S3b-2b では消費しない
- `Unpack` は、行き先が「ブロックの残り」である1つの case の `switch` と同じ意味である
- `TailCall` は、translate が出す要求である。S3b-2b の Perceus は、所有の都合で `let r = call ..; ..; return r` に降格してよい

### インタプリタとランタイム

- 制御は `(関数, ブロック, 文)` である。文の番号がブロックの文の数より小さければその文を、そうでなければ終端を実行する
- `jump` は実引数をすべて読み、行き先の引数に書いてから、行き先の先頭へ進む
- `switch` は case を選び、`Case` のフィールドに書いてから、行き先の先頭へ進む
- `Unpack` は、S3b-2a では消費する `switch` と同じく `take_or_copy` し、タグを確かめる。違えば内部の誤りである
- `Frame::Return` の再開の番地は、実行する側が意味を決める `u64` にする。`eml_runtime` はその中身を解釈しない。インタプリタは、呼び出しの文の `(ブロック, 文)` を入れる。戻ったら、その文の `Let` の変数に結果を入れ、次の文から続ける。`Frame::Return.bind` は削除し、`saved` の鍵は「実行する側が決めるスロットの番号」と文書に書く。将来のバイトコード VM は、ここに自分の `pc` を入れる
- 実行時エラーは `RuntimeError::Fault { fault, function, at: Option<SourceLocation { path, line, column }> }` にする。位置を持つ `Rhs::Extern` が起こした誤りにだけ、その位置を付ける。表示は次の2つである

  ```
  {fault}
    at {path}:{line}:{column}
  ```

  位置がないとき (ヒープ、内部の誤り、位置のない手書きの IR) は、今と同じ ``{fault} in `{function}` `` にする。リークの表示は変えない

### translate

**組み立て方**

- 前から組み立てる。`FnBuilder` は、文を足す (`emit`)、終端を置いてブロックを閉じる (`terminate`)、ラベルを作る (`new_label`)、ラベルをブロックとして置いて今のブロックにする (`place`)、開いたままのブロックに戻る (`reopen`) を持つ。複数のブロックを開いたまま持てる
- 終端はラベルを指し、`finish` が1回のループでラベルを `BlockId` に写す。ブロックは置いた順に番号が付き、この順は前向きの辺だけになる

**ラベルの解決**

ラベルには、そこへ向かう開いたままのブロックの一覧を持たせる。ラベルを持つ式を変換し終えたら、一覧で決める。

| 向かうブロック | 扱い |
|---|---|
| 0 本 | 本体を変換しない |
| 1 本 | そのブロックに戻り、本体をそこで変換する (引数は変数の対応で渡す) |
| 2 本以上 | 各ブロックを `jump L(..)` で閉じ、L を置いて本体を変換する |

`switch` の行き先が、2本以上から入るラベルへ向かうときは、`jump L(..)` だけを持つ辺のブロックを挟む (R3)。前もってラベルの使用を数えることはしない。

**出口**

- 出口は `Return | Jump(Label) | Scrutinize(Ctx)` の3つである
- 値の `if` と `match` は、続きをラベルにして `Jump(続き)` で変換する。続きは上の解決の規則に従う。出口が1つなら続きのブロックを作らず、その値のまま同じブロックで変換を続ける
- `Scrutinize(Ctx)` は、値がそのまま `match` に入る出口である。`Ctx` は、行 (パターン)、枝ごとのラベル、未知の値のためのラベル U (引数1つ)、枝の本体の出口を持つ。枝のラベルの引数は、その枝のパターン変数である

**既知の値**

- 出現を `Occ = Atom(atom, ty) | Con { tag, fields: Vec<Occ>, value: Option<Atom> }` で表す。タプルはタグ 0 のコンストラクタである
- 出現は、scrutinee の位置にあるコンストラクタの適用とタプルのリテラル、`Atom::Tag` と `Atom::Int`、同じ関数の中で出した `let x = con ..` から作る。最後のものは、変数から引数への対応を builder が覚える。変数は1回だけ定義され、定義は使う位置を支配するので、この対応は正しい
- `match`、分解の `let`、関数の引数のパターンは、同じ関数で scrutinee から出現を作る
- 決定木で、コンストラクタやリテラルの頭が既知の出現に当たれば、その場で case を選ぶ
- 値全体を束縛する枝 (`Bind`) では、その葉でだけ `con` を作る。使われない `con` は DCE が消す

**`match`**

1. `decide` が決定木 (`Switch | Unpack | Leaf { arm, bound }`) を作る
2. 決定木を出力する
3. 枝を枝の順に変換する
   - 葉が0の枝は出さない
   - 葉が1つの枝は、その葉のブロックで変換し、枝の変数を出現のアトムに対応させる (別名の `let` を作らない)
   - 葉が2つ以上の枝は、葉から枝のブロックへ `jump` し、枝のブロックを木の後に置く

- `Unpack` を出すのは、コンストラクタが1つだけの型で、フィールドが1つ以上あるときだけである
- フィールドのない唯一のコンストラクタは、`()` と同じくワイルドカードとして扱い、何も出さない

**タプルの列**

- scrutinee が (`Annot` を外して) タプルのリテラルなら、その要素を決定木の列にする。複数の等式を持つ関数で HIR が作るタプルも含む
- 値全体を束縛する行があれば、その葉でだけタプルを作る

**分解の `let` と引数**

- 決定木の葉が1つなら、今のブロックに `Unpack` を並べ、続きもそのブロックで変換する
- 葉が複数なら、葉から続きのブロックへ `jump` し、束縛した変数をその引数にする

**条件と case-of-case**

- 条件は、`Bool` の文脈 `[True] -> T | [False] -> F` を持つ `Scrutinize` で変換する (T と F のラベルは引数を持たない)
- `if c a b` は `match c with True -> a | False -> b` と同じ扱いである。`&&`、`||`、入れ子の `if`、条件の位置の `match` は、`Bool` の値を作らずに分岐する
- `match`、分解の `let`、条件の scrutinee が (`Annot` と先頭の `let` を外して) `if`、`match`、それで終わるブロックなら、scrutinee を `Scrutinize(ctx)` で変換する
- `let x = S` の直後の本体が `match x` である形も含める。枝が `x` を使うなら、`x` を枝のラベルの追加の引数で渡す
- 出口の値 e では、まず e から出現を作る
  - 決定木が、未知の値を調べずに1つの枝に行き着けば、その枝のラベルへフィールドのアトムを引数にして `jump` する。コンストラクタは作らない。枝が値全体か既知の部分を束縛するなら、その場で `con` を作って渡す
  - そうでなければ e を作り、`jump U(e)` で U へ行く
- scrutinee を変換し終えたら、U を解決の規則で決める。0 本なら `switch` を出さない。1 本ならそのブロックに戻って決定木全体を出す。2 本以上なら U を置く
- 次に、枝のラベルを枝の順に解決する。枝の本体は `Ctx` の出口で変換するので、入れ子になった case-of-case もそのまま組み合わさる
- 束縛と `match` の間に文がある形、呼び出しの結果、translate の後に現れる機会は扱わない。最後のものは、将来のインライン化と一緒に入れる jump threading としてロードマップに残す

**末尾呼び出し**

`finish` の最後に、ブロックを見て構造的に作る。構文の規則には頼らない。

- 文がなく、終端が `return p` で、引数が `[p]` だけのブロック b があれば、b へのすべての `jump b(a)` を `return a` に置き換え、b を消す。番号の大きいブロックから処理するので、連鎖も1回でたたまれる
- `let x = <Rhs::Call>` の後の終端が `return x` なら、`TailCall` にする。対象は直接の呼び出し、`mask` で分けた `apply` の最後の塊、余った引数を渡す呼び出し、`handle`、`perform`、`resume`、引数のないトップレベルの値である

**位置**

- `eml_core_ir::lower(hir, typed, entry, &SourceFiles)` にする (`eml_types::check` と同じ形)
- `Loc` は、呼ばれる側の範囲の先頭 (演算子のトークンか extern の名前) から作る
- `SourceFiles::add` が行の先頭の表を作り、`line_col` は二分探索で行を見つけ、その行の中だけ文字を数える
- 値として使う extern (参照、部分適用) は、参照する場所ごとにラッパー `<外側>$externN` を作り、その場所の位置を持たせる。共有の `extern$X` は削除する
- `$lambdaN`、`$handleN`、`$externN` の番号は、HIR の式の ID の順で振る (前もって HIR を1回なめる)。変換の順には依らない

### パス

- `Pass` は `{ Translate, Contract, Perceus }` にする。順序は、translate (`finish` で末尾呼び出しを作る) → verify (scope の段) → contract → verify (scope の段) → Perceus → verify (所有の段) である
- **contract** (`contract.rs`)
  - 使われない純粋な `Let` を消す。純粋なのは `ConstString`、`Con`、`MakeClosure`、`Pure` の extern で、`MayFail` の extern は消さない
  - ブロックを後ろから、文を後ろから見る1回のパスで、関数全体の使用の数を使う。これで不動点に達する。debug ビルドでは、2回目のパスが何も変えないことを確かめる
  - 消した後、そのブロックの末尾にだけ、末尾呼び出しの規則をもう一度当てる
- **生存解析:** ブロックを後ろからたどる1回のループで、ブロックごとの入口の生存集合を疎な集合で側の表に持つ。IR には書かない
  - `switch` の出口: 各 case の (行き先の入口 − case のフィールド) の和に、scrutinee を足したもの
  - `jump` の出口: (行き先の入口 − 引数) に、実引数を足したもの
- **Perceus:** ブロックを前からたどり、その場で書き換える
  - `switch` の行き先の入口は、(`switch` の前の所有 − scrutinee) に RC の対象のフィールドを足したものを所有する。S3b-2a では、行き先が scrutinee を使うなら `switch` の前で `dup` する (今と同じ)
  - 合流するブロックの入口は、(入口の生存集合 − 引数) のうち RC の対象と、RC の対象の引数を所有する
  - ブロックの先頭で、所有していて死んでいる変数を `decref` する。各文の後で、その文が定義した RC の対象の変数のうち死んでいるものを `decref` する。`jump` の前で、渡さない所有を `decref` する
  - `saved` は、呼び出しの後で生きている変数から、結果の変数を除いたものである
  - 終端を「文と新しい終端」に置き換える編集を、その場でできる形にしておく (S3b-2b の `TailCall` の降格のため)
- **verifier:** ブロックを前からたどる1回のループで、次を行う。生存解析は使わない
  - 支配木を Cooper-Harvey-Kennedy の方法で求める。番号の順が前向きなので1回で済む
  - 変数ごとに定義の位置 (ブロック、文。引数は -1) を記録し、支配を前順と後順の番号で調べる
  - 所有の多重集合は、合流を待つブロックごとに疎に持つ
  - R1〜R8 と引き継ぐ検査を行う
- **再帰の深さ:** パス、verifier、pretty、parse、インタプリタ、`Drop`、`Clone`、`Debug` のどれも、プログラムの大きさに比例して Rust のスタックを使わない。残るのは、translate の HIR の式の入れ子 (E0013 が抑える) と、`decide` のパターンの大きさの分 (status.md の既知の制限) だけである

### テキストの形

```
fn f(x.0: int) -> int {
  let c.1: enum = extern Prelude.<(x.0, 10)
  switch c.1 { #0 -> b1, #1 -> b2 }
b1:
  jump b3(x.0)
b2:
  let t.2: int = extern Prelude.+(x.0, 1) @"main.em":2:20
  jump b3(t.2)
b3(t.3: int):
  return t.3
}
```

- 関数: `fn 名前(引数) -> repr { … }`
- 変数: 束縛する位置は `名前.N: repr`、使う位置は `名前.N`
- ラベル: 行頭の `bN:` か `bN(引数):`。入口のブロックにはラベルを付けない
- 文: `let x.N: r = <rhs>`、`unpack v.N #t(f.N: r, ..)`、`dup v.N`、`decref v.N`
- 右辺
  - `call f(..)`、`apply c(..)`、`perform E.op(..)`、`perform never E.op(..)`、`resume k(v, s)`
  - `handle E(init, body) { 節 } return r` (構造体の欄の順)
  - `closure f(..)`、`con #t(..)`、`const ".."`、`extern X.y(..) [@"path":l:c]`、`drop a`
  - 呼び出しの前に `mask [E, ..]` を書ける。Perceus の後は `save [..]` が付く
- 終端: `return a`、`tail <呼び出し>`、`jump bN(..)`、`switch a { #t(f.N: r, ..) -> bN, 3 -> bN, "s" -> bN, _ -> bN }`
- 位置: 既定の pretty は位置を出さない。`pretty_with_positions` が出す
- parser は構文だけを検査する。後ろ向きの `jump`、引数の数の誤り、見えない変数の使用などの不正な IR を書け、verifier のテストに使える
- pretty、parse、pretty の往復は、元と同じになる

## 対象外

- S3b-2b の項目 (上の「S3b-2 の分け方」)
- 呼び出しの結果に対する case-of-case と、translate の後に現れる case-of-case。将来のインライン化と一緒に、jump threading として入れる
- ループ化 (後ろ向きの辺)、バイトコード VM、ネイティブ化
- `decide` のパターンの大きさの分の再帰

## テスト

### 足すテスト

- **translate**
  - 出口が1つの値の `if`、2本の `jump` が入る続き
  - 条件: `&&`、`||`、入れ子の `if`、条件の位置の `match`、辺のブロック、到達しない枝、静的に選ばれる頭
  - case-of-case: `let (lo, hi) = if ..`、`let found = if .. then Some v else None; match found`、`match (if .. then Err .. else Ok n)`、値全体を束縛する枝、入れ子の case-of-case
  - 既知の値: `let` で束縛したコンストラクタ、リテラルのタプルの分解
  - タプルの列: 複数の等式の関数、値全体を束縛する行
  - そのほか: 共有された枝、フィールドのない唯一のコンストラクタ (`Unpack` を出さない)
  - 末尾呼び出し: `let y = if ..; y`、`let y = match ..; let z = y; z`、`let (y, _) = g x; y` (タプル全体を返さない)
  - 位置: 演算子、名前の extern、場所ごとのラッパー
  - 番号: 式の ID の順で振ること
- **verifier:** R1〜R8 のそれぞれ。`jump` が入る `switch` の行き先、引数を持つ `switch` の行き先、フィールドのない `Unpack`、`obj` でない `Unpack`、`jump` の Repr の不一致、保存しない呼び出しの後で使う RC でない変数
- **Perceus:** 片方の辺のブロックでだけ死ぬ変数、使われない `Unpack` のフィールド、使われない呼び出しの結果、case-of-case で枝に渡したフィールドを後でも使う形
- **contract:** `tests/simplify.rs` の DCE の3件、消した後の末尾呼び出し
- **インタプリタ:** 位置付きのエラー、検証しない IR でのタグや数の違う `Unpack`
- **そのほか**
  - `error.rs`: 位置付きの表示と、位置のない表示
  - `eml_diagnostics`: 大きなファイルでの `line_col`
  - 往復: 2万の条件を持つ IR で、debug ビルドの pretty → parse → pretty
  - UI の run テスト: `data Token = Token` を、引数、`let`、case の枝で使う

### テストの変更

[テスト戦略](../../implementation/testing.md) の「テストの変更の運用」の種類で書く。

**成否の変更 (テストの削除)**

計画で1件ずつ挙げ、理由を付ける。

- `tests/simplify.rs` のテスト。1件ずつ次のどれかの行を付ける
  - translate のテスト (名前を示す) へ移す
  - contract のテストへ移す
  - 対象の関数が `main` から届かない空のテストなので削除する
- `src/compact.rs` の6件 (`compact` を削除するため)
- `src/text.rs` の、後継のない構文のテスト (join、`captures`、`^` の印)。文字列のエスケープや不正な入力の誤りなど、新しい構文に引き継げるものは期待値の変更にする
- `src/builder.rs::a_builder_makes_a_function_with_its_join_index`、`src/lib.rs::children_and_atoms_come_in_a_fixed_order`、`src/verify.rs` の木の共有と木の外の式のテスト、`tests/verify.rs::captures_out_of_order_are_rejected` (それぞれ対象の構造を削除するため)

**期待値の変更 (範囲)**

- Core IR のダンプのスナップショットと、IR のテキストや IR を直接組み立てるテストのすべて
  - `eml_core_ir` の `tests/translate.rs`、`tests/perceus.rs`、`tests/verify.rs` と `src/` の単体テスト
  - `eml_interp` のテスト (`ir/mask.core` を含む)
  - `eml_cli/tests/api.rs` の pretty
  - 対象の関数に届かない空のテストは、`main` から呼ぶ形か IR のテキストに書き直す
- run-fail のスナップショット7本 (`tests/ui/run-fail/{basics,files}`) と `crates/eml_cli/tests/cli.rs` の stderr の比較。位置が付き、関数の名前が消える
- 深さのテスト
  - `a_long_run_of_if_statements_is_verified_in_linear_time` (`tests/verify.rs`) と、`eml_interp/tests/run.rs` の文の `if` の連鎖のテストを、`step : Bool -> <IO> Unit` に `if b then ..` を並べる形に書き直し、`switch` が N 個出ることも確かめる。今の `if True` は定数としてたたまれ、`switch` が出なくなるためである
  - 1000 の共有された枝のテストは、scrutinee を引数にする
- `eml_test_support/tests/support.rs::core_until_stops_after_the_named_pass` と `eml_cli/tests/api.rs::compile_until_stops_after_the_named_pass`。join と `captures` の代わりに、DCE を証拠にする
- `tests/verify.rs` の合流のテストを、R6 のテストに書き直す
- `eml_runtime` のヒープのテストのうち、`Frame::Return` の値を比べるもの

**機械的な追随**

- `Pass::Simplify` を `Pass::Contract` にする改名
- `RuntimeError::Fault` のリテラルに `at: None` を足すこと
- UI テストのコメント (`missing_file_through_pipe.em`、`division_by_zero.em`、`join_points.em`)
- `line_col` の内部の変更
- `eml_core_ir::lower` の呼び出し元に `&SourceFiles` を渡すこと

**変わらないもの**

- run と check-fail の UI テストの出力
- `eml_interp/tests/scaling.rs` の上限 (数は上限の中で変わりうる)

## 確認の手順

- `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check`
- 既定でない feature の組み合わせ: `cargo clippy -p eml_cli --no-default-features` と `--features types`、`--features core`。`eml_test_support` の同じ組み合わせと `--features hir`
- `cargo test -p eml_cli --test integration citations`
- `nix build`

## 更新する文書

作業の途中で直すもの。

- `docs/spec/core-ir.md` を書き直す。ブロックの列、R1〜R8、並列な代入、`Unpack`、フィールドの束縛の規則、Repr の表と `ret`、位置、実行時エラーの形、要求としての `TailCall`、パスの順と各パスの境界の不変条件、生存解析を使わない verifier
- `docs/implementation/testing.md`: Core IR のテキストの形、置き場所の表の記述、パスごとの規則、`runtime/` の分類の説明
- `docs/future/evidence-passing.md`: join point を「引数を持つ合流のブロック」に、「`simplify` などのパス」を「translate と Perceus の間のパス」に直す
- `docs/README.md` の行
- `CLAUDE.md`: パイプライン、`Pass` の名前、`eml_diagnostics` を使う crate の一覧、Core IR の記述

段の終わりに1回で直すもの。

- `docs/implementation/architecture.md`: `eml_core_ir` の内部、builder、パス、インタプリタの制御と再開の番地、`eml_core_ir` から `eml_diagnostics` への依存
- `docs/implementation/status.md`: `extern$` の制限と、共有された枝の深さの制限を、それぞれを確かめるテストが入ってから消す
- `docs/future/roadmap.md`
  - 段の表と S3b-2 の節を 2a と 2b に分け、S3b-2a の節は段の終わりに削除する。S3b-2b の節に、Repr の確定 (多相な位置の規則、`Float`)、消費しない `Switch` と `Unpack`、`TailCall` の降格を書く
  - S3b-2 の論点 (縮約パスの範囲、E0013 と深さ、別名の伝播と定数の畳み込み) を閉じる
  - 処理系の項目に「ブロックの引数を通した既知のコンストラクタの jump threading」を足す。インライン化と一緒に入れること、R3 を守る作り方 (辺のブロックと合流のブロック、番号の振り直し、到達しないブロックの掃除)、最初の段として合流の引数のタプルを展開する規則を書く
  - ループ化の項目を、印を付けたループの頭へ戻る後ろ向きの辺を許す形で書き直す
  - backtrace の項目の例を、場所ごとのラッパーの名前に直す
- `docs/superpowers/specs/2026-10-07-redesign-design.md` の S3 の記述 (「ブロック木」を「前向きの辺だけの基本ブロックの列」に)

最後に、`grep -rn -e 'join point' -e simplify -e captures -e アリーナ -e 'extern\$' -e 'ブロック構造の木' docs CLAUDE.md` が、意図して残す記述だけを出すことを確かめる。

## 完了の条件

- UI テストの出力が変わらない。例外は、位置が付く run-fail の7本と、新しい UI テストである
- 深さのテストが、`switch` を実際に出す条件のまま、debug ビルドで通る
- `compact`、`captures`、`joins`、`simplify`、`CExprId` がなくなっている
- verifier が生存解析を使わない
- 上の確認の手順がすべて通る
