# リファクタリング R7: 単一ファイルの前提をなくす作り替えの設計

位置づけ: 作業用の設計文書。R7 を終えたら、残す価値のある内容を `docs/spec/`、`docs/implementation/architecture.md`、`docs/implementation/testing.md`、`docs/implementation/status.md` に移し、この文書は削除する。

## 目的と範囲

S2 (モジュール、import、`pub`、レコードなど) の前に、処理系から単一ファイルの前提をなくす。出発点は [status.md](../../implementation/status.md) の「R7 で直す項目」の8項目である。R7 では互換性を考えず、観測できるふるまいの変更も許す。

### R7 と S2 の境目

R7 は構造の作り替えに限る。2つ目のモジュールは Prelude だけで、モジュールをまたぐ ID、宣言の結果、ファイルを持つ診断の位置、定義に付く fixity は、Prelude で確かめる。

- import と修飾名は、R7 の後も E0004 のままにする。`pub` は、Prelude が公開する名前を選ぶために R7 で実装する
- ローダは、Prelude と入口のファイルを読む session までを作る
- 次のものは S2 に回す。import をたどるローダ、複数ファイルのテストの fixture、ディレクトリを1件とする UI テスト、import の循環を許すかとモジュールの根をどこにするかの決定

### CST と E0004 の範囲

名前と経路のノード、import、`pub`、`::` のパターンまでを CST に組む。補間とコマンドリテラル (lexer のモードが要る) と、レコードとリスト (レイアウト規則3と `with` の衝突を S2 で解く) は、CST を組まずに今の層で E0004 を出す例外とする (2章)。

### 見直しで確かめた不具合

| # | 現象 | 原因 | 直す回 |
|---|---|---|---|
| 1 | `<M.E>` が E0004 にならず、「cannot find effect `M`」(E1002) になる | `ast::Effect::name()` が最初の `UIDENT` を取る。CST に名前の経路のノードがない | R7a |
| 2 | 操作と同じ名前の関数を定義すると、E1003 の後に handler の節で E1001 が連鎖する | `ItemScope::define_function` が `HashMap::insert` で操作を上書きする | R7b |
| 3 | `\| y :: ys -> …` のパターンが E0004 にならず、「cannot find constructor `::`」(E1001) になる | `::` のパターンの変換に E0004 の分岐がない | R7a |

### 方式

item を単位にした、プログラム全体のパイプラインにする (rust-analyzer の item tree と DefMap の分け方を、eml の規模に合わせたもの)。

モジュールごとに検査して、依存するモジュールのインタフェースを借りる方式は採らない。eml はシグネチャが必須なので、型の情報が宣言の間を流れる経路はスキームしかない。型検査は R5 の後、宣言ごとの `Shape` (段0)、関数ごとの本体 (段1)、SCC ごとの Kind (段2) の粒度で動いており、モジュールの境目を使わない。モジュールごとの方式にすると、次の3つの負担が生じる。

- 相互再帰がモジュールをまたぐと SCC もまたぐので、import の循環を許せなくなる
- `eml_types` が、自分の表と依存の表の2系統で引くことになる
- モジュールのインタフェースとしての型検査の出力を、新しく設計する必要がある

item を単位にすれば、型検査は今の構造のまま、ID がプログラム全体のものになるだけで済む。import の循環を許すかどうかも、`DefMap` の作り方だけの問題になる。

採らなかったものは次のとおり。

- salsa を今入れること。段階をクエリの形にしておけば、後で載せ替えられる。LSP を作るまでは手間が大きい
- モジュールごとに Core IR を作ってリンクすること。インタプリタでは得るものがない
- HIR の位置を source map に移すこと。[ロードマップ](../../future/roadmap.md) のとおり、クエリ化のときに行う
- 単一のアリーナにプログラム全体の item を置くこと。モジュールの変換が共有のアリーナを書き換えるので、段階が純粋な関数でなくなる

### 回の分け方

R7 は R7a → R7b → R7c → R7d → R7e-1 → R7e-2 → R7f の順に、それぞれ計画と実装のサイクルで進める。R7e は、互いに独立した2つの変更 (呼び出しの飽和の処理と平らな `Switch`) を別の回に分け、R7 の後始末を R7f にした。

| 回 | 範囲 | 章 |
|---|---|---|
| R7a | 構文: `NAME`、`NAME_REF`、`PATH`、import の CST、E0004 を HIR に寄せる、不具合1と3 | 2 |
| R7b | `ItemTree`、`DefMap`、item ごとの変換、プログラム全体の ID、Prelude のモジュール、重複と fixity、lang item、session、intrinsic を本体のない関数にする、不具合2 | 1、3、4.2 |
| R7c | 型検査の出力 (`TypedProgram`、`DeclType`)、由来の `Span`、`main` を入口の引数にする | 5 |
| R7d | `IO` のエフェクト、Prelude の本体、`|>` と `<|` の脱糖をやめる、入口から届く関数だけの変換と `Prelude.` の名前、`Bool` のタグ | 4.3〜4.6 |
| R7e-1 | Core IR: 呼び出しの飽和の処理を1つの補助関数にまとめる | 6.1、6.2 |
| R7e-2 | Core IR: 平らな `Switch` | 6.3 |
| R7f | 後始末: この文書の内容を `docs/` に移して削除する、参照の張り替え、`architecture.md` の簡素化 | 7.5 |

R7c を R7d より先にするのは、Prelude の本体が入ると、持ち越しの由来が Prelude の中の位置を指すようになるためである。由来がファイルを持つ形を先に作っておく。R7b に intrinsic を入れるのは、Prelude を普通のモジュールにした時点で `Res::Builtin` を残すと、名前の解決の経路が2つになるためである。

## 1. 全体の構成

### 1.1 段階と入口

| 段階 | 入口 | 単位 |
|---|---|---|
| 構文 | `eml_syntax::parse(FileId, &str) -> (Parse, Vec<Diagnostic>)` (変えない) | ファイル |
| item の収集 | `eml_hir::item_tree(FileId, &ast::SourceFile) -> (ItemTree, Vec<Diagnostic>)` | ファイル |
| スコープ表 | `eml_hir::def_map(&[ItemTree]) -> (DefMap, Vec<Diagnostic>)` | プログラム |
| item ごとの変換 | `eml_hir::lower(&DefMap, &[ItemTree]) -> (hir::Program, Vec<Diagnostic>)` | item |
| 型検査 | `eml_types::check(&hir::Program) -> (TypedProgram, Vec<Diagnostic>)` | 宣言、本体、SCC |
| Core IR | `eml_core_ir::lower(&hir::Program, &TypedProgram, FunctionId) -> Program` | プログラム |

- `ItemTree` はファイルごとの宣言の要約で、名前を解決しない (3.1)
- `DefMap` は、モジュールの一覧 (`ModuleId` → ファイル、モジュール名) と、モジュールごとのスコープ表を持つ。重複の判定と fixity の付け先の決定は、すべてここで行う (3.2、3.3)
- 3つの入口は公開し、`eml_hir` の結合テストから個別に呼べるようにする
- `eml_core_ir::lower_until` も、`lower` と同じ引数に変える。入口の関数の `FunctionId` を引数で受け取るのは R7c からである (5.3)

### 1.2 ID

- `ModuleId` は `DefMap` の中のモジュールの番号である。Prelude はつねに最初のモジュールにする
- item の ID は `FunctionId { module: ModuleId, local: Idx<Function> }` の形にする。`TypeDefId`、`EffectId`、`OperationId`、`ConstructorId` も同じ形である
- `ExprId`、`PatId`、`LocalId` は、今と同じく本体の中の番号のままにする。本体は関数の ID で引くので、プログラム全体で一意にする必要がない
- `hir::Program` に `Index<FunctionId>` などを実装し、`program[id]` で引けるようにする

### 1.3 HIR のプログラムの形

```rust
pub struct Program {
    pub modules: Arena<Module>,          // ModuleId で引く
    pub lang: LangItems,
}

pub struct Module {
    pub file: FileId,
    pub name: String,                    // "Prelude"、"Main" など
    pub items: Items,                    // functions (シグネチャまで)、types、constructors、effects、operations
    pub bodies: ArenaMap<Idx<Function>, Body>,
}
```

- インタフェースと本体の分割は、`Function` から `body` を外して `Module::bodies` に置く形で行う。型検査の段0は `items` だけを読み、段1は本体を1つずつ読む
- `Module::builtins` はなくす (4.2)
- HIR の pretty は、入口のモジュールだけを表示する

### 1.4 Prelude と session

- Prelude のソースは、`eml_hir` が `pub const PRELUDE_SOURCE` として公開する。中身は今の `crates/eml_hir/src/prelude.em` を育てたもの (4.1) である
- 呼び出す側 (CLI と `eml_test_support`) は、Prelude を `SourceFiles` に普通のファイルとして登録する。表示するパスは `Prelude.em` である。`FileId::PRELUDE` はなくす
- Prelude の中の位置を指す診断の secondary (「defined here」など) は、ほかのファイルと同じく表示する
- `eml_cli` の lib API は `Session` を中心にする。`execute` は今のままにする

```rust
pub struct Session { files: SourceFiles, prelude: FileId }

impl Session {
    pub fn new() -> Session;                                  // Prelude を登録する
    pub fn add_file(&mut self, path: impl Into<String>, text: impl Into<String>) -> FileId;
    pub fn check(&self, entry: FileId) -> Vec<Diagnostic>;
    pub fn compile(&self, entry: FileId) -> Compiled;
    pub fn files(&self) -> &SourceFiles;                      // 診断の表示に使う
}
```

- `main` は入口のモジュールの `main` だけを探す (`Program::main()`、5.3)。Prelude には `main` を置かない
- Prelude だけを検査して診断が出ないことを、`eml_cli` のテストで確かめる。Prelude の誤りは処理系の誤りだからである

### 1.5 `pub` と名前の解決の順

- `pub` を R7 で実装する。Prelude は、ユーザーに見せる名前を `pub` で選ぶ。`pub` でない Prelude の item (`negate` など) は、ユーザーには見えず、lang item として処理系からだけ引ける
- ユーザーのモジュールに書いた `pub` も受け付ける。import する側がまだないので、効果はない
- 名前の解決の順は「ローカルの束縛 → 自分のモジュールのトップレベル → import で修飾なしにした名前 → Prelude の `pub` の名前」とする。今の [modules.md](../../spec/modules.md) には「自分のモジュールのトップレベル」がないので足す。ユーザーの定義が Prelude の名前を隠せるという今のふるまいは保たれる

## 2. R7a 構文

### 2.1 名前のノード

定義の名前と参照の名前を別のノードにする (rust-analyzer の `Name` と `NameRef`)。

| ノード | 中身 | 現れる位置 |
|---|---|---|
| `NAME` | `LIDENT`、`UIDENT`、`( OP )`、`CONOP` のどれか1つ | シグネチャ、等式の関数名、`data` の名前と型引数、コンストラクタ、`effect` の名前と型引数、操作の宣言、fixity の演算子、`import ... as X`、束縛のパターン (`BIND_PAT` の中) |
| `NAME_REF` | `LIDENT` か `UIDENT` の1つ | `PATH` のセグメント、import の並びの名前 |
| `PATH` | `NAME_REF (DOT NAME_REF)*` | `PATH_EXPR`、`PATH_TYPE`、`APP_TYPE`、`CON_PAT`、`EFFECT`、import のモジュールの経路 |

- `PATH` はセグメントを平たく並べる。eml の修飾名は「モジュールの経路 + 最後の名前」の形しかないので、入れ子の `PATH` は要らない
- 演算子の列の `OP` と、`OP_REF` (`(+)`) の中の `OP` はトークンのままにする。eml の文法に修飾付きの演算子はなく、HIR はトークンの文字列で引く
- フィールドの名前 (`t.x`、`(.x)`) も今のままにする。レコードの CST は S2 で組み直す
- 型付き AST では、`name()` が `ast::Name` を、`path()` が `ast::Path` を返す。`Path::segments()` は `NAME_REF` の並びを返し、`Path::is_qualified()` で修飾の有無を判定する。`ast::Effect::name()` を `path()` に置き換え、不具合1を直す

### 2.2 CST まで組む構文

- `import_item` は `IMPORT_ITEM { 'import' PATH ('as' NAME)? IMPORT_LIST? }` にする。`IMPORT_LIST` は `IMPORT_NAME { NAME_REF | '(' OP ')' , ('(' '..' ')')? }` の並びである
- `pub` は、今と同じく item の中の `PUB_KW` である
- `data_item` の `'=' alts` を省略できるようにする変更は、E1025 と一緒に R7b で入れる (3.5)
- `::` のパターンは、中置のコンストラクタのパターンとしてすでに組まれているので変えない

### 2.3 E0004 を出す層

方針は「パーサは CST を組み、意味を与える最初の段階 (HIR) が E0004 を出す」の1つにする。

| 構文 | 今 | R7 の後 |
|---|---|---|
| import | parser (`ERROR` で読み飛ばす) | CST を組み、HIR (`item_tree`) が E0004 |
| `type` の宣言 | parser | HIR (`item_tree`) が E0004 |
| 修飾名 (式、型、パターン、row のエフェクト) | HIR。row のエフェクトは E1002 になる (不具合1) | HIR が `PATH` を見て E0004 |
| `::` (式) | HIR | 変えない |
| `::` (パターン) | E1001 になる (不具合3) | HIR が E0004 |
| 浮動小数、文字、複数行の文字列、raw 文字列 | parser | HIR が E0004 |
| フィールドの参照、`(.x)` | HIR | 変えない |
| 補間、コマンドリテラル | lexer と parser | 変えない (例外。lexer のモードと一緒に S2 と S3 で作り直す) |
| レコード、リスト | parser (`ERROR`) | 変えない (例外。レイアウト規則3と `with` の衝突を S2 で解く) |

- 2つの例外は、[grammar.md](../../spec/grammar.md) の「実装の段階」に書く
- E0004 の文言は、出す層が変わっても今と同じにする。位置が変わるもの (import は今 `ERROR` の範囲を指すが、HIR では import の item の範囲になる) は種類1の変更として扱う (7.1)

## 3. R7b 名前解決

### 3.1 `ItemTree`

- トップレベルの item をソースの順に集める。シグネチャと等式は、名前で1つの関数の item にまとめる。E1004、E1005、E1018、E1019、E1020 は名前を解決しなくても判定できるので、ここで出す
- 各 item は、名前、名前の範囲、`pub`、AST のノードを持つ。`data` はコンストラクタの名前を、`effect` は操作の名前を、それぞれ子として持つ
- fixity の宣言は、演算子の名前と範囲だけを持つ。付け先は `DefMap` が決める

### 3.2 `DefMap` と重複の規則

モジュールごとに、値 (関数、操作、コンストラクタ) と型 (型、エフェクト) の2つの名前空間を持つ。重複の扱いは次の3つの規則だけにする。

1. 名前ごとに、その名前の定義をソースの順にすべて持つ。最初の定義がその名前の定義である。2つ目以降には E1003 を出し、「first defined here」で最初の定義を指す。重複した定義の本体も変換して検査するので、本体の中の誤りも報告する
2. 重複した `data` のコンストラクタと、重複した `effect` の操作は、「使えない」印を付けて登録する。それらを参照した位置は、診断を出さずに `Missing` にする。最初の定義の型と比べて、「expected `T`, found `T`」のような連鎖を出さないためである。今の `ValueItem::Unusable` を操作にも広げたものである
3. 種類を指定して引く位置では、名前の定義のうち、その種類で最初のものを引く。パターンの先頭はコンストラクタを、handler の節の先頭は操作を引く ([modules.md](../../spec/modules.md) の「名前の解決」)

- 名前を引く順は 1.5 のとおりで、種類を指定した引き方もこの順にたどる。ユーザーが `println` という関数を定義していても、節の `| println s k` は自分のモジュールに `println` という操作がないので、Prelude の操作を引いて E1009 になる
- 不具合2は、規則1と3で直る。後で定義した関数に E1003 が出て、節は操作を引く。関数を先に定義した場合も、節は操作を引く
- `data` とエフェクトの型引数の重複 (E1003) は、宣言の中の名前の表で、同じく最初の定義を残す

### 3.3 fixity は定義に付ける

- fixity の宣言の演算子は、自分のモジュールの値の名前空間で引き、見つかった定義 (関数、コンストラクタ、操作) に fixity を付ける。見つからなければ E1022、同じ演算子の2回目の宣言は E1021 である
- 演算子の列の組み直し (`climb`、`climb_pat`、セクション) は、演算子の名前を解決した先の定義の fixity を使う。宣言がなければ `infixl 9` である。`ItemScope` の文字列の表 `fixities` と `prelude_fixities` はなくす
- 今の Prelude には、定義のない演算子の fixity がある。扱いは次のとおり
  - `&&`、`||`、`|>`、`<|`: Prelude で関数として宣言し、lang item にする。HIR は、解決した先がこの lang item の二項演算を、今と同じく脱糖する (`&&` と `||` は `if`、`|>` は評価の順の印 `evaluate_first`)。ユーザーの定義に解決した場合は、普通の呼び出しにする。R7b では本体のないシグネチャにし、R7d で本体を書く (4.4)。二項演算はつねに脱糖し、`(&&)` などの演算子の参照はラムダに脱糖するので、R7b の間にこの4つが Core IR まで届くことはない。R7d で、`|>` と `<|` の脱糖をやめ、Prelude の関数の普通の呼び出しにする (4.4)。`&&` と `||` は、短絡して評価するために脱糖を続ける
  - `::`: Prelude から fixity の宣言を外す。S2 で `List` のコンストラクタとして定義するときに戻す。R7 の間 `::` は E0004 なので、外しても誤りの式の木の形が変わるだけである

### 3.4 lang item

- `DefMap` が、Prelude のモジュールから名前で lang item を引く。名前の表は `eml_hir` の Rust の表で、`Int`、`String`、`Unit`、`File`、`Bool`、`True`、`False`、`IO`、`negate`、`==`、`!=`、`&&`、`||`、`|>`、`<|` である。`|>` と `<|` は、R7d で脱糖をやめるときに表から外す (4.4)
- Prelude は処理系と一緒に配るソースなので、lang item が見つからなければ panic にする
- lang item は `pub` でなくても引ける。前置の `-` は、`pub` でない `negate` に脱糖する

### 3.5 `=` のない `data`

- Prelude の `Int`、`String`、`Unit`、`File` は、`=` のない `data` として書く (`pub data Int`)。見え方は `pub` で決める
- `=` のない `data` は、Prelude の中では intrinsic の型である。`File` が `Lin` であることと、`Unit` が空のレコードであることは、今と同じく型検査が lang item で判定する
- ユーザーのモジュールに書いた `=` のない `data` は、新しい診断 E1025 (`MISSING_CONSTRUCTORS`、「`data` にコンストラクタがない」) にする。今は構文エラーなので、ふるまいの変更である

## 4. Prelude (R7b と R7d)

### 4.1 Prelude の中身 (R7d の後)

```
pub data Int
pub data String
pub data Unit
pub data File
pub data Bool = | False | True

pub effect IO where
  println : String -> Unit
  open : String -> File
  read_all : File -> (File, String)
  close : File -> Unit

-- intrinsic (等式のないシグネチャ)
pub show_int : Int -> String
negate : Int -> Int
pub (+) : Int -> Int -> Int          -- -、*、/、%、<、<=、>、>=、++ も同じ
pub (==) : a -> a -> Bool            -- != も同じ

-- eml で書く関数
pub not : Bool -> Bool
not True = False
not False = True
pub (&&) : Bool -> Bool -> Bool
a && b = if a then b else False      -- ||、|>、<|、>>、<< も本体を書く

-- fixity (今の表から :: を除く)
pub infixr 0 <|
...
```

### 4.2 intrinsic (R7b)

- Prelude の中の、等式のないシグネチャが intrinsic である。ユーザーのモジュールでは、今と同じく E1005 である
- intrinsic は、HIR の上では本体のない普通の関数で、`Res::Function` に解決する。`Res::Builtin`、`Builtin`、`BUILTINS`、`Access`、`Module::builtins`、`TypedModule::builtins`、`Decl::Builtin`、`ValueItem::Builtin` はなくす
- intrinsic の引数の数は、シグネチャの一番外側の矢印の数である。本体のある関数の引数の数は、今と同じく等式の引数の数である
- 名前から実装を引く Rust の表は、`eml_core_ir` に1つだけ置く。対応は次のとおり

| Prelude の名前 | 実装 |
|---|---|
| `show_int`、`negate`、算術、比較、`++` | `PrimOp` |
| `==`、`!=` | 型検査が決めた比べ方 (`Equality`) で `PrimOp` を選ぶ |
| `IO` の操作 | `IoOp` (`Rhs::Io`)。R7d から、`IO` は宣言したエフェクトになり、操作の名前から `IoOp` を引く (4.3) |
| `>>`、`<<` | R7d までは今の `Compose` |

- `eml_core_ir` の単体テストで、Prelude の intrinsic と `IO` の操作のすべてが表に行を持ち、表の行がすべて Prelude にあることを確かめる
- `negate` は eml で書かずに intrinsic のまま残す。`0 - x` と書くと、`i64::MIN` のあふれの実行時エラーが、ユーザーの関数ではなく `negate` の中で起きたと報告されるためである
- `==` と `!=` の比べ方の決定 (`check/equality.rs`) は、`Builtin::IntEq` の代わりに lang item で呼ばれる側を判定する

### 4.3 `IO` を宣言したエフェクトにする (R7d)

- R7b の間は、`println` などは Prelude の intrinsic の関数 (シグネチャに `<IO>` を持つ) である。`IO` は、今の `builtin_items` と同じく処理系が Prelude のモジュールに登録する、操作のないエフェクトである。R7d で、どちらも Prelude の `effect IO` の宣言に置き換える
- `println`、`open`、`read_all`、`close` は `once` の操作になり、`Res::Operation` に解決する
- 型検査では、`IO` の多重度が操作の多重度の最大 (`Once`) から決まる。`Context` の「`IO` は `Once`」の特別扱いはなくす。`File` を受ける操作は「単相な `Lin` の型の引数」なので、操作の引数を `Unr` に固定する規則には当たらない ([effects.md](../../spec/effects.md))
- E1009 は「節の操作が lang item の `IO` の操作か」で判定する。文言は今のままにする。`Builtin::is_io_operation` はなくす
- `DefMap` が Prelude に足していた合成の `IO` (`SyntheticEffect`、`PRELUDE_EFFECTS`) と、E1009 のための `LangItems::io_operations` はなくす
- Core IR は、`IO` の操作の呼び出しを、今と同じく `Rhs::Io` に変換する。`call_operation` は、操作のエフェクトが lang item の `IO` なら `perform` ではなく `Rhs::Io` を作る。`IoOp` は操作の名前から引き (`IoOp::from_name`)、intrinsic の表から `IO` の4行を外す。`println` を値として使ったときは、操作を包む関数 (`op$println`) を作り、その本体も `Rhs::Io` にする。インタプリタは変わらない
- `eml_core_ir` の単体テストで、`IO` の操作がすべて `IoOp` を持ち、`IoOp` がすべて Prelude の `IO` の操作であることを確かめる
- `main` のシグネチャの検査は、今と同じく lang item の `IO` で判定する
- 関数の型の表示 (`println : String -> <IO> Unit`) は変わらない。操作の型も、最後の矢印にエフェクトを付けて表示しているためである

### 4.4 Prelude の本体 (R7d)

- `not`、`&&`、`||`、`|>`、`<|`、`>>`、`<<` を eml で書く。`>>` は `f >> g = fn x -> g (f x)` の形で書き、引数の数は2になる。`f >> g` の値は `f` と `g` を捕まえたクロージャで、Kind は今の部分適用と同じく `f` と `g` の Kind 以上になる
- Core IR の `Lowering::Compose` と、それを包む関数 (`builtin$>>`、`builtin$<<`) はなくす
- `f >> g` の中で `f` か `g` が失敗した場合は、今と同じく `f` か `g` の名前で報告する
- `not` を eml で書くので、`PrimOp::Not` とインタプリタのその分岐はなくす
- `|>` と `<|` は脱糖をやめ、Prelude の関数 (`x |> f = f x`、`f <| x = f x`) の普通の呼び出しにする。実装を簡潔に保つためである。評価の順は変わらない。`x |> f a` は呼び出し `(|>) x (f a)` になり、引数を左から評価するので、今と同じく `x`、`f`、`a` の順に評価する。評価の順の印 `evaluate_first` (`hir.rs`、`lower/ops.rs`、`lower/expr.rs`、`eval.rs`、`pretty.rs`) と、lang item の `pipe` と `apply` はなくす
  - 代わりに次のものが変わる。`|>` と `<|` の型の誤りは、`|>` の引数の不一致として報告する。たとえば `"a" |> show_int` は、`"a"` の「expected `Int`, found `String`」ではなく、`show_int` の引数の型の不一致になる (OCaml の `|>` と同じ形である)。`x |> f a` の Core IR は、`f a` のクロージャを作ってから `Prelude.|>` を呼ぶ形になり、インタプリタではクロージャの確保と呼び出しが1回ずつ増える
- `&&` と `||` は、短絡して評価するために、今と同じく `if` に脱糖する。本体は型検査だけを受け、Core IR には届かない

### 4.5 Core IR に入れる関数と名前 (R7d)

- Prelude に本体が入ると、本体のある関数をすべて変換する今の `translate` は、使わない Prelude の関数まで Core IR に入れる。そのため、`translate` は入口の関数から届く関数だけを変換する。HIR の本体の中の関数の参照 (`Res::Function`) を、入口の関数からたどって集める。ユーザーの関数と Prelude の関数を区別しない。関数を参照されたときに初めて作る仕組み (包む関数) と合わせて、使わない item は Core IR に入らない
- Prelude の関数の Core IR の名前には `Prelude.` を付ける (`Prelude.not`)。ユーザーが Prelude と同じ名前の関数を定義しても、Core IR の名前が重ならないことを構造で保証するためである。テキストの IR は `.` を名前の一部として読む
- 入口から届かない関数を定義したテストでは、その関数が Core IR から消える。期待値の変わるテストは 7.1 に挙げる

### 4.6 `Bool` のタグ (R7d)

- `eml_core_ir` の定数 `FALSE` (0) と `TRUE` (1) は、Core IR での `Bool` の表し方として残す。タグは Prelude の `data Bool = | False | True` の宣言の順で決まり、Prelude は処理系と一緒に配るソースなので、この順を変える理由がないためである
- HIR の `lower/mod.rs` の、タグを確かめる `assert_eq!` はなくす。代わりに、Prelude の `False` と `True` のタグが `FALSE` と `TRUE` に等しいことを、`eml_core_ir` のテストで確かめる
- 採らなかった形: Core IR の変換が `program[lang.true_ctor].tag` からタグを引き、インタプリタには `Program::bool_tags` で渡す形。比べるプリミティブのためにインタプリタまでタグを運び、テキストの IR にもその欄を足すことになる

## 5. R7c 型検査の出力

方式は item を単位にするので、スキームをほかのモジュールへ渡すために出力を損失なしにする必要は強くない。それでも次の理由で R7 のうちに直す。

- Prelude の本体が入ると、持ち越しの由来が Prelude の中の位置を指す。由来にファイルが要る
- 出力を宣言ごとの結果 (`Shape` と `KindScheme`) にそろえておけば、クエリ化のときに宣言ごとのクエリの結果として使える。REPL で前の入力の宣言を検査し直さずに使うときも、同じ結果を使い回せる
- 表示のための `Scheme` が後の段階への出力を兼ねていて、`TypedModule` の役割がわかりにくい。今の `Scheme` への変換では、矢印の線形性、多重度の制約、変数どうしの制約、持ち越しの由来、Kind 変数の同一性が落ちる

### 5.1 出力の形

```rust
pub struct TypedProgram {
    pub decls: HashMap<Decl, DeclType>,
    pub bodies: ItemMap<Function, BodyTypes>,   // BodyTypes は今のまま
}

pub enum Decl {
    Function(FunctionId),
    Operation(OperationId),
    Constructor(ConstructorId),
}

pub struct DeclType {
    /// 後の段階が読む、矢印の線形性のない型。
    pub ty: Type,
    pub(crate) shape: Shape,
    pub(crate) kinds: KindScheme,
}
```

- `ty` は、検査の最後に `shape.export(context)` で1回だけ作る。`Type` は型とエフェクトの名前を持ち、名前は `Context` から引くので、`Shape` だけでは `Type` を組み立てられないためである。後の段階は `typed.decls[&decl].ty` で読む
- `shape` と `kinds` は crate の外から読めないようにする。後の段階が Kind を読まないという今の規律 ([architecture.md](../../implementation/architecture.md) の「`Table::export`」) を保つためである
- `Scheme` と `KindConstraint` は `dump` の中だけで使う表示の型にして、公開をやめる
- `Decl` は今の crate 内の `kind::problem::Decl` を公開にしたものである
- `main` は型検査の結果ではないので、`TypedProgram` に持たせない (5.3)
- 採らなかった形は次のとおり
  - `ty(&self, program: &Program, decl)` で呼ぶたびに組み立てる形。本体の型 (`BodyTypes`) は名前を持つ `Type` のままなので、宣言の型だけを組み立て直しても得るものがない
  - `Type` から名前をなくし、表示するときに `Program` から引く形。同じ名前の別の型を修飾して区別できる (REPL で `data T` を定義し直すと起きる) が、`Display`、診断の文言、`dump` の経路をすべて変えることになる。S2 で同じ名前の別の型の表示を決めるときに一緒に扱う ([status.md](../../implementation/status.md) の「次の作業の注意点」)。そのときも後の段階は `ty` の欄を読むだけなので、使う側は変わらない

### 5.2 由来とファイル

- 位置にファイルを持たせる型 `Span { file: FileId, range: TextRange }` を足す。使うのが `eml_types` だけなので、`eml_types` の中に置く
- ファイルが変わりうる位置だけを `Span` にする。`KindOrigin { span: Span, reason }`、`CarriedInner { span: Span, label }`、`Provenance::Unattributed(Span)` の3か所である
  - スキームから制約を複写するときは、由来を参照した位置のものに置き換える (`kind/solve.rs` の `copy_scheme`)。呼ばれた側の位置が残るのは、`CarriedThrough` の `inner` だけである。R7d で Prelude に本体が入ると、ここが Prelude の中を指す
  - `KindReason` の中の位置 (`first`、`second`、`clause`、`binding`、`init`、`UnusedPath` の範囲、`DropFix` の位置) は、どれも由来を作った本体の中にある。`TextRange` のままにし、`KindOrigin.span` と同じファイルにあるという決まりを型のコメントに書く
- 報告 (`check/report.rs`) は、入口のファイルの決め打ちをやめ、`origin.span.file` と `inner.span.file` を使う
- 違反を並べるキー (`report_violations`) にファイルを足す。プログラム全体の違反を1つの列に並べるので、範囲だけで並べると別のファイルの違反が混ざるためである
- `Context` (データ型の Kind、名前、多重度) と SCC の分割は、R7b-2 でプログラム全体から作るようにした。本体の検査の診断も、R7b-2 で本体のあるモジュールのファイルを使うようにした

### 5.3 `main`

- `eml_hir::Program::main() -> Option<FunctionId>` を足す。入口のモジュールの `main` を返す
- `eml_types` は、E2004 の検査にこれを使う
- `eml_core_ir::lower` と `lower_until` は、入口の関数を `entry: FunctionId` の引数で受け取る。`Session::compile` が `program.main()` を引き、`None` なら E2003 を足し、あれば `lower` に渡す。REPL では、`main` の代わりにその回の式から作った関数を渡せる
- `eml_test_support` の `core`、`core_until`、`run` は、中で `program.main()` を引いて渡す。各 crate のテストの呼び出しの形は変わらない

### 5.4 `dump`

- `dump` は入口のモジュールの宣言だけを表示し、Prelude の宣言は表示しない。`kinds:` の行の書き方は変えない
- `Type` の表示 (`Display`) は変えない

## 6. R7e Core IR

### 6.1 呼び出しの飽和の処理を1つにまとめる (R7e-1)

今の変換は、既知の呼ばれる式への呼び出しを、ユーザーの関数 (`call_known`)、intrinsic (`call_intrinsic`)、操作 (`call_operation`)、コンストラクタ (`call_constructor`) の4つの関数で扱う。どれも「引数が足りなければ包む関数のクロージャ、ちょうどなら命令、余れば結果への `Apply`」という同じ場合分けを、別々に書いている。これを1つの補助関数 `saturate` にまとめる。

- `saturate` は、引数の数、引数、足りないときに包む関数を作る手順、ちょうどのときの命令を作る手順を受け取る。足りなければ包む関数のクロージャにし、ちょうどなら命令を束縛し、余れば命令の結果に残りの引数を `Apply` する
- `call_head` は、呼ばれる式の種類ごとに次の2つだけを決めて、`saturate` に渡す

| 種類 | 引数の数 | 足りないときの包む関数 | ちょうどのときの命令 |
|---|---|---|---|
| ユーザーの関数 | `program.arity(target)` | その関数 | `call` |
| intrinsic | シグネチャの引数の数 | `builtin$…` | `prim` (`==` と `!=` は型検査が決めた比べ方で選ぶ) |
| 操作 | 操作の引数の数 | `op$…` | `operation_rhs` (`perform` か `io`) |
| コンストラクタ | フィールドの数 | `con$…` | `con` |

- 束縛の変数の名前 (`t`、コンストラクタの `d`) は今のままにする。Core IR のスナップショットは変わらない
- `call_intrinsic`、`call_operation`、`call_constructor` は `saturate` にまとめてなくす。値として使う位置の変換 (`atom`) は呼び出しではないので変えない
- 包む関数の3つの作り方 (`wrapper`、`operation_wrapper`、`constructor_wrapper`) と、満ちた呼び出しを変換の時点で命令にすることは、今のままにする

### 6.2 採らなかった形: どの item も関数にして、規則 I と `prune` で戻す

intrinsic、操作、コンストラクタも本体を持つ Core IR の関数にし、変換はすべてを関数の呼び出しにして、`simplify` の規則 I (印のある関数の直接の呼び出しを本体の1行で置き換える) で命令に戻し、参照がなくなった関数をパス `prune` で取り除く形は採らない。次の理由による。

- 今の変換は、満ちた呼び出しを変換の時点で命令にしているので、規則 I と同じことをすでにしている。重複していたのは飽和の場合分けだけで、6.1 の補助関数で1つにできる
- 規則 I、`inline` の印、`prune` (関数を取り除くと `FnIdx` の参照をすべて付け替える)、比べ方ごとの関数 (`eq$Int` など) が要り、書いて保つコードが増える
- 変換の直後で止めるスナップショット (34件) のほとんどで、`prim +` が `call builtin$+(…)` になる
- R7d から、変換は入口から届くトップレベルの関数だけを作るので、`prune` の仕事は I の後始末だけになっていた

### 6.3 平らな `Switch` (R7e-2)

```rust
Switch { scrutinee: Atom, cases: Vec<Case>, default: Option<CExprId> }
Case { pattern: CasePattern, fields: Vec<VarId>, body: CExprId }
enum CasePattern { Tag(u32), Int(i64), String(u32) }   // String は文字列定数の番号
```

- `Arm` はなくし、`Case` にする
- リテラルの列の `match` は、比べるプリミティブと `Bool` の `Switch` の連なりではなく、リテラルの case と `default` (ワイルドカードの行だけの行列) を持つ1つの `Switch` にする。深さが1になるので、リテラルが1000個の `match` でも debug ビルドのスタックがあふれない。`translate/pattern.rs` の `compare` と `if_equal` はなくす
- コンストラクタの列でも、default の行列に行く残りのコンストラクタをまとめて `default` にする。今は残りのコンストラクタごとに枝を作り、join point に飛ばしている。その join point と `jump` を作る処理はなくなる。`if` とタプルは、今と同じく `Tag` の case にする
- `Switch` は今と同じく scrutinee を消費する。`String` のリテラルの `Switch` は、比べ終わってから文字列を解放する。今は比べるたびに `dup` しているので、その分が減る
- `simplify` の K1 と B2 は、分かっているタグを `cases` から探し、なければ `default` に進む。子をたどる規則 (F、B3、B5 など) と、生存解析と `compact` は、`default` もたどる
- verifier は、1つの `Switch` の case の種類がそろっていること、リテラルの `Switch` に `default` があること、同じ case が2回ないことを確かめる
- インタプリタは、case を順に比べ、一致するものがなければ `default` に進む
- テキストの形は `switch x { #0 -> …, 1 -> …, "a" -> …, _ -> … }` にする
- `perceus.rs` と `verify.rs` のコメントにある「E0013 が深さを抑える」という誤った記述を消す。[status.md](../../implementation/status.md) の「次の作業の注意点」のうち、リテラルの `match` の深さの限界の項目を直す

## 7. テスト、文書、完了の条件

### 7.1 テストの変更

各回の計画の全体の制約は、[testing.md](../../implementation/testing.md) のとおり「期待値は、このプランで名前を挙げたテストだけを変える。期待値を変えない機械的な追随は許す」とする。

種類1 (振る舞いの変更) は、この spec の承認で前もって合意したものとし、[test-changes.md](../../implementation/test-changes.md) に記録する。

| 回 | 種類1の変更 |
|---|---|
| R7a | E0004 を出す層が変わる構文 (import、`type`、浮動小数、文字、複数行と raw の文字列) の診断の位置。これらのテストを `eml_syntax` の結合テストから `eml_hir` の結合テストへ移す。不具合1と3の UI テストを `check-fail/not-yet-supported/` に足す |
| R7b | 不具合2のテスト (`crates/eml_hir/tests/effects.rs` の重複のテスト) を、E1001 が出ないことまで確かめる形に強める。重複の規則の UI テストを `check-fail/names/` に足す。`=` のない `data` が構文エラーから E1025 になる |
| R7d | Prelude の中の位置を指す secondary が、診断に現れることがある (`|>` を通る E3006 など)。`|>` と `<|` の型の誤りの診断が、`|>` の引数の不一致になる (4.4)。これらを期待値に持つ `eml_types` のテストと UI テストを、計画で列挙する |

種類2 (内部表現) は、次のスナップショットが変わる。各回の計画で、変わるテストの名前を列挙する。

| 回 | 変わるスナップショット |
|---|---|
| R7a | `eml_syntax` の CST (`shape` の56件のほとんど)、`src/parser/tests.rs` |
| R7b | `eml_hir` の pretty のうち ID の表示を含むもの |
| R7d | `eml_core_ir` の Core IR のうち、入口から届かない関数を定義したもの (下見で数えて、`perceus.rs` 9件、`simplify.rs` 27件、`translate.rs` 18件、`eml_test_support` の `support.rs` 1件)、`not`、`>>`、`<<`、`|>`、`<|` を使うもの、`println` を値として使うもの。ソースから作る Core IR の表示の先頭に増える `effect IO { … }` の1行 (`pretty` は操作のあるエフェクトをすべて表示し、`IO` を外す特別扱いは入れない)。`>>` と `<<` を使う関数の `eml_types` の `dump` の `kinds:` の行 (`>>` の本体の持ち越しの制約)。`eml_hir` の pretty のうち、`evaluate_first` の印を含むもの |
| R7e-1 | なし (Core IR のスナップショットは変わらない) |
| R7e-2 | `eml_core_ir` の `switch` を含むスナップショット (約30件)、リテラルの `match` と、残りのコンストラクタを join point に送っていた `match` のスナップショット、`verify.rs` と `eml_interp/tests/data.rs` の `switch` を含むテキスト |

種類3 (機械的な追随) は、ID の形と入口の引数の変更に合わせたテストの組み立ての書き換えである。R7c では、`eml_types/tests/check.rs` の `signatures` と `constructors` を `decls` から読む形にし、`eml_test_support` の `Checked::typed` の型の名前を変える。R7c は観測できるふるまいを変えないので、種類1と種類2の変更はない。

新しく足すテストは次のとおり。

- R7b: `ItemTree` と `DefMap` の結合テスト (重複の3つの規則、fixity の付け先、名前の解決の順)。Prelude だけを検査して診断が出ないことのテスト
- R7c: `Program::main()` の `eml_hir` の結合テスト (入口のモジュールに `main` がある場合、ない場合、`main` が操作の名前である場合)。由来が Prelude の中を指す持ち越しの診断が、`Prelude.em` の位置を表示するテスト (R7d で Prelude の本体が入ってから足す)
- R7d: 入口から届く関数だけを変換することと、Prelude の関数の `Prelude.` の名前 (`translate.rs`)。`println` を値として使ったときの Core IR (`translate.rs`)。Prelude の `Bool` のタグが `FALSE` と `TRUE` に等しいこと (`eml_core_ir`)。`IO` の操作と `IoOp` の対応 (`eml_core_ir` の単体テスト)。Prelude の本体の中の E3001〜E3004 が `Prelude.em` を指すこと (`eml_types/tests/modules.rs`。R7c で残した)
- R7e-1: なし。既存のテストで、ふるまいが変わらないことを確かめる
- R7e-2: 平らな `Switch` の verifier の誤りのテスト (`verify.rs`。case の種類が混ざる、リテラルの `Switch` に `default` がない、同じ case が2回ある)、K1 と B2 が `default` に進むテスト (`simplify.rs`)、リテラルが1000個の `match` を debug ビルドで実行するテスト (`eml_interp/tests/run.rs`。ソースはテストの中で生成する)

### 7.2 `eml_test_support`

- `parse`、`lower`、`check`、`core`、`core_until`、`run` は、今と同じくソースの文字列を1つ受け取り、Prelude を自動で登録する。各 crate のテストの呼び出しの形を保つためである
- 複数ファイルの fixture は S2 で足す

### 7.3 文書の更新

| 文書 | 更新の内容 | 回 |
|---|---|---|
| `spec/grammar.md` | E0004 の層の方針と2つの例外 (R7a)、`data` の `'=' alts` の省略 (R7b) | R7a、R7b |
| `spec/modules.md` | 名前の解決の順に「自分のモジュールのトップレベル」を足す。重複の3つの規則 | R7b |
| `spec/declarations.md` | `=` のない `data` (Prelude の intrinsic の型と E1025)、fixity は定義に付くこと、`::` の fixity を S2 に回すこと | R7b |
| `spec/diagnostics.md` | E1025 | R7b |
| `spec/effects.md` | `IO` を Prelude で宣言したエフェクトにすること | R7d |
| `spec/declarations.md`、`spec/expressions.md` | `|>` と `<|` を脱糖せず Prelude の関数として呼ぶこと (標準の演算子の表、「関数適用」の評価の順、HIR の脱糖の一覧) | R7d |
| `spec/core-ir.md` | 入口から届く関数だけを変換すること、Prelude の関数の `Prelude.` の名前、`builtin$>>` と `builtin$<<` がなくなること | R7d |
| `spec/core-ir.md` | 平らな `Switch` (`Case`、`CasePattern`、`default`、テキストの形、verifier の規則) | R7e-2 |
| `implementation/architecture.md` | 段階の入口、ID、`ItemTree` と `DefMap`、`TypedProgram` と `DeclType`、由来の `Span`、`lower` の `entry` 引数、session。R7f で、[ロードマップ](../../future/roadmap.md) の「文書の簡素化」のとおり細部の説明を減らす | 各回、R7f |
| `implementation/testing.md` | テストの地図と `eml_test_support` | 各回 |
| `implementation/status.md` | 回ごとに更新する。S2 に回したもの (import をたどるローダ、複数ファイルの fixture、ディレクトリを1件とする UI テスト、import の循環とモジュールの根の決定) を S2 の項目に移す。R7c で、同じ名前の別の型の表示の項目に、`Type` から名前をなくす案 (5.1) を書き足す | 各回 |

### 7.4 完了の条件

1. 1〜6章の内容が入っていること。不具合1〜3が UI テストで確かめられていること
2. 次のものがコードから消えていること: `Res::Builtin`、`Builtin`、`BUILTINS`、`Access`、`Module::builtins`、`TypedModule::builtins`、`Decl::Builtin`、`ValueItem::Builtin`、`FileId::PRELUDE`、`ItemScope` の `fixities` と `prelude_fixities`、`Lowering::Compose`、`PrimOp::Not`、`evaluate_first`、`LangItems` の `pipe`、`apply`、`io_operations`、`SyntheticEffect`、`call_intrinsic`、`call_operation`、`call_constructor`、`Arm`、`translate/pattern.rs` の `compare` と `if_equal`、`TypedModule`、公開の `Scheme`、`check/report.rs` の入口のファイルの決め打ち
3. `cargo test`、`cargo clippy --all-targets`、`cargo fmt` が通ること
4. 型検査の性能のテスト (`crates/eml_types/tests/scaling.rs`、release ビルド) の比が6以下のままであること。Prelude をプログラムに含めて検査するようになるためである

### 7.5 R7f 後始末

- この文書のうち残す価値のある内容 (item を単位にした方式と、採らなかった形とその理由) を `implementation/architecture.md` に移す。`docs/spec/` に入れた決定は重ねて書かない
- コードのコメントと文書から、この文書を指す参照を、内容を移した先に張り替える (`grep -rn 2026-10-06-refactor-r7-design crates docs` で探す)
- この文書と R7 の計画の文書 (`docs/superpowers/plans/` の R7 の回のもの) を削除する。完了した作業の計画は削除する運用 ([status.md](../../implementation/status.md) の「完了した作業」) に合わせる
- `implementation/architecture.md` を、[ロードマップ](../../future/roadmap.md) の「文書の簡素化」のとおり、コードの細部の説明を減らす形にする
- `implementation/status.md` の R7 の行を完了にし、「完了した作業」の表に R7 の行を足す。ロードマップの「文書の簡素化」の項目を、済んだ分だけ直す
