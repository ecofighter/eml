# S3b-2c-2 Core IR v2 の境界 (設計)

位置づけ: 作業の設計。再設計のサブプロジェクト S3b-2c-2 の spec である。出発点は [ロードマップ](../../future/roadmap.md) の「S3b-2c-2 Core IR v2 の境界」と、[全体設計](2026-10-07-redesign-design.md) の S3b の記述である。決まったことは段の終わりに `docs/` の該当文書へ移し、この文書を削除する。

この段では、Repr の違う位置の間で値を渡す規則を決め、スカラーと参照の間の変換を `box` と `unbox` の命令で IR に書く。その命令を入れるパスを translate と縮約の間に置き、末尾呼び出しは縮約だけが作る。verifier は、呼び出しの引数と結果、一様な位置、`tobj` のフィールド、`jump` と `return`、`tail` の結果を比べる。値の意味は変えず、UI テストの出力は変わらない。後のバイトコード VM とネイティブ化は、変数の Repr と `box` と `unbox` から、スロットかレジスタの種類、確保、RC の操作をすべて IR から読める。

設計は次の4つの節からなる。

1. 値の表現と位置の規則
2. `box` と `unbox`
3. パスの順と末尾呼び出し
4. verifier、見張り、段の形、テスト

持ち主が決めたことは次のとおりである。各節はこれを前提に書いている。

- `Int` は、一様な位置との間で明示的に `box` する (1節)
- 関数の値を通るループのフレームを積まないように、末尾の位置で参照を返す呼び出しを持つ関数の `ret` を `tobj` に上げる (T3、1節)。保証は IR の形で定義した末尾の位置の呼び出しについての文で、文を持つ合流のブロックを通る形は保証の外にする
- エフェクトの位置は今は一様にし、操作の宣言した Repr は evidence passing の段に回す (1節)。`never` の操作の `perform` の結果は位置として扱わない
- 命令なしで `tobj` と行き来するスカラーは `unit` だけにする (1節)

この spec の規則の多くは、試作 (8fd85df の上の使い捨ての実装) で確かめた。

- UI テストの出力は1つも変わらなかった。ワークスペースのテストは、下の4節に挙げる期待値の変更を除いて通った
- UI の run と run-fail の 115 本で、静的な `box` は 186 個、`unbox` は 197 個で、55 本のプログラムに現れた。インタプリタの歩数は 26.3% 増え、`rc_*` の回数が変わったのは1本 (+1) だけだった
- 試作で2つの穴が分かった。1つ目は、T3 (1節) がないと、関数の値を通るループがフレームを積むことである。2つ目は、試作の T3 が呼び出しと `return` の間の使われない純粋な `let` を見落とし、その形のループがフレームを積むことである (1節の「T3 と末尾呼び出しの保証」)。この spec は両方を直す
- 設計の途中で、試作の写しに2つの変更を入れて確かめた。トップレベルの関数をその場で一様にしないこと (1節の「一様な関数」) と、T3 を一様化の前に置くこと (3節の「box の挿入の手順」) である。UI テストはすべて通った。`f$boxed` は 1 個から 7 個になり、歩数は UI 全体で 30 増えた。`box`、`unbox`、`tail` の数は変わらず、`peak_objects` が変わったのは2本 (どちらも +1) だった
- 次の規則は試作になく、4節の計画の手順で確かめる。`jump` と `return` を互換で比べること、`never` の操作の `perform` の扱い、`box` と `unbox` の拒否の規則 (試作は `unit` の `box` と、`obj` のオペランドと `unit` の束縛の `unbox` を受け入れた)、変換の段で `box` と `unbox` を拒むこと、縮約が `tail` を作るときの互換の条件、内部の関数の印、`eml_interp` の手書きの IR の8本の書き換えのうち7本である

## 1. 値の表現と位置の規則

### 方式

- 局所の変数はスカラーのまま持ち、呼ぶ側か呼ばれる側が具体的な型を知らない位置 (一様な位置) だけを参照にする。Lean の IR と同じ方式である
  - 変数は、今と同じく具体化した型の Repr を持つ。`int`、`enum`、`unit` の変数は、VM ではタグのないスロットに、ネイティブではレジスタに置ける
  - 一様な位置の Repr は `tobj` である。型変数は `tobj`、関数の型も `tobj` で、関数の値 `&f` は即値のままにする
  - クロージャの呼び出しの規約は変えない。共有されたクロージャへの `apply` は、今と同じく中身を写す (Lean の `pap` と同じ)。クロージャそのものを環境として渡すかは、ネイティブのオブジェクトモデルで決める
- 採らない案
  - 型付きのクロージャの ABI (Koka、Swift): 境界をまたぐたびに包む関数か、シグネチャごとの `apply` が要る。包む関数を通すと、関数の値の同一性を保てず、reuse もできなくなる
  - すべての値を一様にする方式 (OCaml): `Int` の幅を今決めることになり、後の `Float` は呼び出しのたびに確保する

### 位置の規則

位置ごとに、そこへ渡す値に期待する Repr は次のとおりである。正確に合わせるか互換でよいかは、下の「互換の関係」で決める。

| 位置 | 期待する Repr |
|---|---|
| 局所の変数、ブロックの引数 | 具体化した型の Repr (今と同じ) |
| データのフィールド (`con` の引数、case と `unpack` の束縛) | 配置の表の、宣言したフィールドの Repr。型変数と組のフィールドは `tobj` |
| 直接の呼び出し `call g(..)` の引数と結果 | `g` の引数の Repr と `ret` |
| `extern` の引数と結果 | 表の行の `params` と `ret` (S3b-2c-1 の R8) |
| `apply`、`perform`、`resume`、`handle` のオペランドと結果 | `tobj`。ただし `never` の操作の `perform` の結果は値を持たない (下の「`never` の操作」) |
| `closure g(..)` の引数 (捕獲と部分適用) | `g` の引数の Repr。`g` は下の規則で一様なので、一様になる |
| 関数の値として使う関数 (`&g`、`closure g`) の引数と `ret` | `tobj` と互換 (下の「一様な関数」) |
| `return a` | 関数の `ret` |
| `jump bN(..)` | 行き先の引数の Repr |

- translate が決める関数のシグネチャは今と同じである。トップレベルの関数と `op$`、`con$`、`$externN` はスキームから、ラムダ、節、handle の本体は使う所の型から決める。`cont$` と `cont$state` はすべて `tobj` である
- 一様でない関数を一様にするのは、box の挿入のパス (3節) である
- この文書では、`tobj` の位置との間で `box` と `unbox` を要するスカラーの Repr を「箱を要するスカラー」と呼ぶ。今は `int` と `enum` で、後の `Float` の Repr もここに入る。T3、`box` と `unbox` の規則、パスの変換は、この言葉で書く。verifier の文言に並べる Repr の名前も、この集合の1つの定義から作る。`Float` を足すときに、規則を1つずつ探して直さずに済むためである

### 一様な関数

- 関数の値として参照される関数は一様でなければならない。関数の値として参照されるとは、IR のどこかに `&f` があるか、`closure f(..)` の対象であることである。一様とは、引数と `ret` の Repr がすべて `tobj` と互換 (`obj`、`tobj`、`unit`) であることである。`apply` と handler は、関数ごとの Repr を知らずにその関数を呼ぶためである
- 関数には、内部の関数かどうかの印がある。内部の関数は、translate が定義の中から作る関数と、translate が作る補助の関数である。ラムダ、節、handle の本体、`op$`、`con$`、`$externN`、`cont$`、`cont$state` が当たり、S4 のローカルの関数もここに入る。トップレベルの関数 (標準ライブラリを含む) は内部の関数でない。内部の関数を直接呼ぶのは、それを作った定義の中だけである
  - translate が `CoreFn` に印を付ける。テキストの形では、内部の関数を `internal fn main$lambda3(..)` と書く。`f$boxed` の印は `f` と同じにする
- どの関数を一様にするかは、参照と内部の印で決める
  - 値としてだけ参照され、直接は呼ばれない内部の関数は、その場で一様にする。箱を要するスカラーの引数を新しい `tobj` の引数に替え、入口のブロックの先頭で、引数の順に `let p = unbox p'` を置く。箱を要するスカラーの `ret` は `tobj` にする。`obj` と `unit` の引数と `ret` は変えない
  - そのほかの、値として参照され一様でない関数 (直接も呼ばれる内部の関数と、トップレベルの関数) は、形を変えずに残し、一様な関数 `f$boxed` を足す。値の参照 (`&f` と `closure f`) は、すべて `f$boxed` へ向ける
  - `f$boxed` の引数と `ret` は、`f` の箱を要するスカラーを `tobj` にし、`obj` と `unit` はそのままにする。本体は変換の前の形 `let t: <f の ret> = call f(p0, ..)` と `return t` で作り、変換は box の挿入の手順4がほかの関数と同じく入れる。`f` の `ret` が一様なら、縮約がこの呼び出しを `tail` にする
- トップレベルの関数をその場で一様にしない理由: トップレベルの関数の ABI を、その関数の定義と、それが末尾の位置で呼ぶ関数だけで決めるためである (T3 は下)。その場で一様にすると、ほかの定義が後から直接呼ぶかどうかで ABI が変わる。定義ごとに Core IR を保存する REPL (ロードマップ) では古い定義を書き換えることになり、ネイティブ化では標準ライブラリを一度だけ翻訳して使い回せなくなる。内部の関数は、直接の呼び出しがすべて同じ定義の中にあるので、その場で一様にしても困らない。LLVM が引数の形を変えてよい関数を、内部の linkage の関数に限るのと同じ考え方である
- 判定は IR の参照と内部の印だけで決まり、型は読まない。試作では、UI のプログラムで `f$boxed` は1つ (`evaluation_order.em` の `f`) で、その場で一様にしたトップレベルの関数は6つだった (`higher_order.em` の `inc` と `double`、3本のプログラムの `add`、`continuation_in_inner_handle.em` の `act`)。この規則では、その6つにも `f$boxed` が付き、その関数の値を `apply` するたびに直接の呼び出しが1つ増える。試作の写しでは、歩数の増えは UI 全体で 30 だった
- 関数の種類で決める案 (持ち上げた関数をすべて一様にする) は採らない。今の UI では同じ出力になるが、S4 のローカルの再帰関数は lambda lifting して直接呼ぶので、そのループの中に `box` が入る。この段の規則では、直接呼ぶ内部の関数は形を保つ
- 捕獲も一様にする。`closure f(..)` の引数は `f` の引数の Repr に変換し、`f` は一様なので、捕まえた `int` と `enum` は `box` される。捕獲を具体的な Repr のまま持つには、クロージャごとのペイロードの記述子と、関数ごとの一様な入口が要る。その入口は `f$boxed` と同じものなので、2つの仕組みが重なる。試作での費用は、静的に 17 か所、動的に 25 回だった
- エフェクトの位置は一様にする。`perform` の引数と結果、`resume` の継続と値と状態と結果、`handle` の初期の状態と本体と節と `return` の節と結果は、どれも `tobj` である。節と本体の関数は値として参照されるので一様になる
  - 操作の宣言した Repr を使う案 (`perform` の引数と結果、操作の節の引数、`resume` の値を、操作のスキームの Repr にする) は、evidence passing の段に回す。配置の表と同じ考え方で、UI で動的な変換を 220,123 回減らす。一方で、エフェクトの表に操作の Repr を、`resume` に操作の名前を、操作ごとの `cont$` を要する。evidence passing は節の呼び出しの規約を作り直すので、そこで一緒に決める。ロードマップの evidence passing の項目に書く
  - handler ごとに答えと状態を具体的な Repr にする案は採らない。共有の `cont$` と `cont$state` を handler ごとに分け、実行時が呼ぶ節に handler ごとの ABI を持たせることになる。減る変換は動的に 50,208 回で、そのうち 50,004 回が `multi_loop.em` 1本である

### `never` の操作

- `never` の操作の `perform` は値を返さない。節が継続をその場で捨てるので、`perform` の後へ制御が戻らない。そのため、その結果は位置でなく、束縛の Repr は translate が決めたまま (具体化した型の Repr) にする
  - box の挿入は、`never` の `perform` の束縛を受け直さず、その束縛を使う所にも変換を入れない。束縛の使いには制御が届かないからである
  - T3 は `never` の `perform` を見ない。末尾の位置の `perform never` は、どの `ret` とも互換として扱う
  - verifier は、`never` の `perform` の結果を束縛と比べず、末尾の `perform never` の結果を呼び出し元の `ret` と比べない。互換の位置では、`never` の `perform` の束縛の使い (`return` の値、呼び出しの引数など) も位置と比べない。縮約の前の `let t = perform never ..` と `return t` の形も、そのまま `tail perform never` になる
- 理由: 結果を `tobj` とすると、T3 が「確かめて失敗する」形の関数の `ret` を上げる。試作では、`effect_abort.em` の `check_positive` と `checked` が `tail perform never Fail.fail(..)` だけのために `ret` を `tobj` にした。成功する経路で `box n.0` をし、呼び出し元はみな `unbox` と `decref` をした。ネイティブの 64 ビットの `Int` では、成功する呼び出しのたびに箱を確保しうる。`never` の `perform` は戻らないので、フレームを積まず、末尾呼び出しの保証にも関わらない
- 操作の引数は、今までどおり一様な位置である。エフェクトの位置を一様にする決定は変えない
- 戻らない extern (S4 の `exit`) の結果の扱いは、S4 で同じ考え方で決める。ロードマップの S4 の `exit` の項目に書く

### 互換の関係

- 2つの Repr a と b は、次のどれかのとき互換 (`compatible`) である
  - a と b が同じ
  - a と b がどちらも参照 (`obj` か `tobj`)
  - 片方が `unit` で、もう片方が `tobj`
- この関係は推移的でない (`unit` と `obj` は互換でない)。どの検査も実際の2つの位置を比べるだけなので、推移律は使わない
  - 2つの位置を1つにつなぐ変形は、つないだ後の2つの位置が互換なときだけ行う。縮約の末尾呼び出しの規則がこれに当たる (3節)。translate と box の挿入の出力では、型からいつも互換になる
- 正確な位置では Repr が同じでなければならない。次の位置である
  - `extern` の引数と結果 (S3b-2c-1 の R8)
  - 宣言した Repr が `tobj` でないフィールド、タグの `switch` の scrutinee と配置、`unpack` の値 (`obj`)、`con` の束縛と配置 (S3b-2c-1 の R9)
  - `box` と `unbox` のオペランドと束縛 (2節)
- 互換の位置では、互換であればよい。次の位置である
  - `jump` の実引数と行き先の引数
  - `return` の値と `ret`
  - 直接の呼び出しの引数と `g` の引数、結果を束縛する変数と `g` の `ret`
  - `closure g(..)` の引数と `g` の引数
  - `apply`、`perform`、`resume`、`handle` のオペランドと結果を束縛する変数 (相手は `tobj`)
  - 宣言した Repr が `tobj` のフィールド (`con` の引数、case と `unpack` の束縛)
  - `tail` の呼び出しの結果 (直接なら `g` の `ret`、ほかは `tobj`) と、呼び出し元の `ret`
  - 関数の値として使う関数の引数と `ret` (相手は `tobj`)
- `jump` と `return` を互換にする理由
  - 正確にしても、バックエンドが得るものはない。互換な2つの Repr は機械の形が同じだからである (下の「バックエンドは次を守る」)。VM のスロットもネイティブのブロックの引数も、両側を同じに扱える
  - 正確にすると、ロードマップのインライン化と contification が作る形を書けない。たとえば `tobj` を返す `id` を `let t: obj = call id(x)` の所でインライン化すると、`id` の `return x` は `jump bK(x)` になり、`tobj` の x を `obj` の引数へ渡す。`obj` と `tobj` を付け替える命令はなく、引数を `tobj` にすると、その先の `unpack` (`obj` だけを受ける) が通らない。付け替えの命令を足すと、1つの値の書き方が2つになる
  - 今の出力は変わらない。translate は `jump` と `return` を同じ Repr で出し、box の挿入が `return` に入れる変換 (`ret` を上げた関数の `box`) も同じである。今の verifier のテストで、成否が変わるものはない
- 定数の当てはめも、位置の関係に従う
  - 正確な位置では、今の当てはめ (`fits`) を使う。`Int` の定数は `int`、`()` は `unit`、`#N` は `enum` か `tobj`、`&f` は `tobj` に収まる
  - 互換の位置では、これに加えて `()` が `tobj` にも収まる
  - `Int` の定数は `tobj` に収まらない。`tobj` の位置には `let b = box 5` を置いて b を渡す
  - 範囲の段と所有の段では、`&f` は `f` が一様なときだけ収まる (4節の境界の検査)。変換の段では一様かを見ない。translate は、一様でない関数の `&f` を `jump` の実引数に出しうるからである (`let h = if c then double else inc` の `jump b3(&double)`)
  - `#N` が `tobj` に収まるのに `enum` の変数が収まらないのは、定数ならコンパイルの時点で `tobj` の形に書けるからである。`Int` の定数を同じ扱いにしないのは、`box` が確保しうる (ネイティブの 64 ビットの `Int`、後の `Float`) からである

### スカラーと `tobj`

- 箱を要するスカラーは、`tobj` の位置との間で `box` と `unbox` を要する。命令なしで `tobj` と行き来するスカラーは `unit` だけである。`obj` と `tobj` の間は、今と同じく命令なしで渡せる
- 正確な位置 (extern の行と、宣言した Repr が `tobj` でないフィールド) では、`unit` と `tobj` も行き来しない。そこでは Repr が表で決まり、translate の出力も表に合わせて作るからである
- `int` を明示する理由
  - `box` は確保しうる (ネイティブの 64 ビットの `Int`、後の `Float`)。所有している `tobj` を `unbox` した後には `decref` が要る。どちらも Perceus と所有の検査に見えなければならない
  - `Int` を命令なしで通すと、「小さい `Int` は `tobj` の即値」を今決めることになる。`Int` の幅はネイティブ化で決める (ロードマップ)
  - 試作の費用は、インタプリタの歩数で 26.3% である。インタプリタの `box` と `unbox` は値をそのまま渡し (2節)、ヒープの物体の RC を増やさない
- `enum` を明示する理由: タグを `tobj` の中でどう表すかを、バックエンドに任せるためである。Lean も enum を `uint8` にして明示の box を置く。費用は小さく、静的に 22 か所、動的に 20,054 回で、ほぼ `multi_loop.em` だけである
- `unit` を命令なしにする理由: 値が `()` の1つだけなので、変換はデータも確保も RC も持たない。試作では、`unit` の引数を `tobj` にしたときの `decref` の約 230,000 歩がなくなり、`Unit` を返す関数の値を通るループのフレームの伸びも、T3 なしで消えた
- バックエンドは次を守る
  - `unit` の値の機械の形は、`tobj` の即値 `()` と同じにする。`unit` を返す関数の戻り値も、この形で返す。呼び出しの結果、`jump`、`return`、`tail` が、`unit` と `tobj` の間を命令なしでつなぐためである
  - `obj` の値は、そのまま `tobj` の値でもある (今の `tobj` のフィールドの規則と同じ)
  - `tobj` の位置に置いた定数 `#N` は、値が N の `enum` を `box` した値と同じである。`unbox` が、どちらの値も同じに読むためである

### T3 と末尾呼び出しの保証

- 末尾の位置の `apply`、`perform`、`resume`、`handle` の結果は `tobj` である。`ret` が箱を要するスカラーの関数では、その結果を `unbox` するので、呼び出しは末尾呼び出しでなくなる。そのため、関数の値を通るループはフレームを積む。試作の `loop f n = if n == 0 then 0 else f (n - 1)` と `go n = loop go n` では、100,000 回の反復で `peak_objects` (4節) が 2 から 200,002 になった。インタプリタはフレームをヒープに置くのでメモリが O(n) になるだけだが、ネイティブではスタックがあふれる
- T3 の規則: 関数の `ret` が箱を要するスカラーで、その関数の末尾の位置の呼び出しのどれかの結果が `ret` と互換でなければ、`ret` を `tobj` にする
  - 末尾の位置の呼び出しは、IR の形で決まる。1つのブロックの終端が `return x` で、同じブロックに x を定義する `let x = <呼び出し>` があり、その後の文がすべて純粋な `let` (縮約の `pure` が真のもの) であるとき、その呼び出しを末尾の位置の呼び出しという。T3 は translate の出力 (box の挿入の入力) でこれを見る
  - 純粋な `let` を飛ばして見る理由: translate は `let r = f (n - 1) in let s = "unused" in r` を、`let r = apply ..`、`let s = const ..`、`return r` と出す。試作の T3 はこの形を見落とし、`go 1000` で `peak_objects` が 2003 になった。後ろの純粋な `let` の変数は `return` が使わないので、縮約がすべて消し、そこで `tail` を作る
  - 呼び出しの結果は、直接の呼び出しなら呼ばれる関数の `ret`、ほかは `tobj` である。`never` の操作の `perform` は見ない (上の「`never` の操作」)
  - 上げた関数を末尾の位置で呼ぶ関数も、同じ規則で上げる。不動点まで伝える
  - `unit` の `ret` は上げない。`unit` の関数の末尾の位置の呼び出しの結果は、型から `unit` か `tobj` に決まり、どちらも互換だからである
  - 上げた関数の直接の呼び出し元は、結果を `unbox` で受ける。Perceus がその後に `decref` を置く
- 保証: 末尾の位置の呼び出しをたどって元の関数に戻る輪の上にある末尾の位置の呼び出しは、どれも `tail` になる。結果に変換の要る末尾の位置の呼び出しは、普通の呼び出しになる。この文を、上の末尾の位置の呼び出しの定義と一緒に、spec の変換の規則の末尾呼び出しの段落に書く
  - 輪の辺は、直接の呼び出しなら呼ばれる関数へ向き、`apply`、`perform`、`resume`、`handle` ならどの関数へも向きうるとみなす。保証の理由はそれらの結果が `tobj` であることしか使わないからである
  - 保証は IR の形についての文である。呼び出しの値が、文を持つ合流のブロックを通って `return` に届く形 (`let r = if n == 0 then 0 else f (n - 1) in let s = "unused" in r`) は、末尾の位置の呼び出しでなく、保証の外にある。今の main でも同じで、translate の転送は文のない合流のブロックにしか効かない。試作では、この形のループで反復ごとに積むフレームが1つから2つになった (100,000 回で `peak_objects` が 100,003 から 200,003)。どちらも反復の数に比例し、係数だけが変わる。`status.md` の「深さと性能」に、輪の上にない残りと一緒に書く
  - 後のパスもこの保証を保つ。所有の都合の降格 (ロードマップの借用パラメータ) は、輪の上の `tail` に当てない。借用の推論は、輪の上の `tail` が渡す引数の仮引数を所有にして、降格を避ける (Lean の `ownParamsUsingArgs`)。降格してよいのは輪の上にない末尾呼び出しだけである。ロードマップの借用パラメータの項目に書く
- 保証の理由
  - T3 の後に結果が互換でない末尾の位置の呼び出しは (`never` の `perform` を除く)、`ret` が `tobj` の関数から、`ret` が箱を要するスカラーの関数への直接の呼び出しだけである。呼び出し元の `ret` がスカラーなら、T3 が上げているので互換になる。呼び出し元の `ret` が `obj` なら、呼ばれる側の `ret` は `obj` か `tobj` である。translate の出力では、返す変数の Repr が `ret` と同じで、直接の呼び出しの結果の変数は呼ばれる側の宣言した型の具体化なので、呼ばれる側の `ret` はその Repr か `tobj` になるからである。パスは `obj` の `ret` を変えない
  - そのような呼ばれる側 g が輪の上にあるとする。g の `ret` を s (箱を要するスカラー) とする。g の末尾の位置の呼び出しは不動点で互換なので、`ret` が s の関数への直接の呼び出しである (`apply` などの結果 `tobj` は s と互換でない)。輪をたどると、輪の上の関数の `ret` はすべて s になる。輪の上には `ret` が `tobj` の呼び出し元もあるので、矛盾する
  - T3 は `f$boxed` を作る前に走る (3節)。`f$boxed` は直接呼ばれず、`ret` が `tobj` なので、`f$boxed` を足した後の IR でも同じ議論が成り立つ
  - 試作では、輪の上にない残りは UI で 14 か所で、どれも「`$handleN` の本体が、`Int` を返すトップレベルの関数を末尾の位置で呼ぶ」形だった。輪をたどらないので積むフレームは有界で、`peak_objects` の増えは最大で 3 だった
- 呼ばれる側も上げる案は採らない。保証には要らず、呼ばれる側のほかの呼び出し元すべての ABI を変える
- 輪の上の関数だけを上げる案も採らない。値の参照を含む呼び出しのグラフの強連結成分が要るのに、試作で上げた関数は 14 本のプログラムで 18 個だけで、動的な費用の差も測れなかった
- Lean の `InferBorrow.ownParamsUsingArgs` が、末尾呼び出しを保つために ABI を変えるのと同じ考え方である

## 2. `box` と `unbox`

### 命令

- `Rhs::Box(Atom)` と `Rhs::Unbox(Atom)` を足す
- テキストの形は `let b.3: tobj = box n.2`、`let b.4: tobj = box 5`、`let n.2: int = unbox b.3` である。スカラーの種類は、`box` ではオペランドの Repr から、`unbox` では束縛の Repr から決まるので、型の引数を書かない
- `parse` は、`drop` と同じく1つのアトムを読む。新しい誤りの文言はない。Repr の誤りは verifier が報告する。誤りを含む IR も読み戻して verifier に渡すためで、`extern` の引数と同じ扱いである
- `Stmt` にしない理由: 定義する変数は1つだけである。`Rhs` なら網羅の `match` に足す所が約6か所で済み、`Stmt` にすると9か所になる

### オペランドと束縛

- `box a` の a は、箱を要するスカラーの変数か、`Int` の定数である。束縛の Repr は `tobj` である
  - `unit` の変数、`()`、`#N`、`&f`、参照の変数は拒む。どれも命令なしで `tobj` に収まるので、1つの値の書き方を1つに保つ。`closure` と `con` の規則と同じ考え方である
- `unbox a` の a は、`tobj` の変数である。束縛の Repr は箱を要するスカラーである
  - `obj` の変数は拒む。`obj` はつねにヒープの物体を指し、スカラーを入れた値にならない
  - 定数と、`unit` の束縛も拒む
- この規則は、範囲の段と所有の段 (4節) で確かめる。変換の段は `box` と `unbox` をそもそも拒む

### 所有

- `box` はオペランドを消費し、所有した `tobj` を定義する。オペランドはスカラーか定数なので、RC の対象ではない
- `unbox` はオペランドを読むだけで、所有権を受け取らない。`switch` の scrutinee と `unpack` の値と同じ「読む」使いである。借りたフィールドも読める
- `Rhs::for_each_consumed` を足す。`Unbox` のオペランドを除き、`for_each_atom` と同じアトムを返す。`Stmt::for_each_consumed` は、`Let` でこれを呼ぶ。生存解析と縮約は今のまま `for_each_atom` を使い、読む使いも使いに数える
- Perceus: `let x = unbox w` の後で、w がこの時点で所有している RC の対象で、この文の後で死んでいれば、文の直後に `decref w` を置く。`unpack` の直後の規則の、読む使いの版である
  - この段では、生きている RC の対象は、フィールドも含めてこの時点で所有になっている (フィールドは case の行き先の入口か `unpack` の直後で所有になる)。そのため、この `decref` は所有を手放すだけである。借用パラメータを入れたら、借りている w には置かない
  - w がこの後も生きていれば何も置かない
- verifier の所有の段は、`unbox` のオペランドを読む使いとして確かめる (見えて有効であること)
- 消費する `unbox` (読むことと `decref` を1つの命令にする案) は採らない。ロードマップの「`unbox` は読む使い」に反し、後のフィールドを死ぬ所で手放す形で、`release` の前にまだ借りているフィールドを `unbox` で読む形を使えなくする。試作では `unbox` 197 個のうち 195 個の後に `decref` が付いた。この数は、その後の作業の理由としてロードマップの Perceus の項目に書く

### 縮約

- `box` と `unbox` は純粋で、使われなければ縮約が消す。使われない `box` を消しても確保が1つ減るだけで、`unbox` は読むだけなので、どちらも評価の順を変えない

### インタプリタ

- `box` と `unbox` は値をそのまま渡す。`Value` が自分の種類を持つためである
- どちらも、値が `Value::Obj` なら内部の誤りで止める。文言は `internal error: a box of a heap object` と `internal error: an unbox of a heap object` である (どちらも後ろに `` in `f` `` が付く)
  - R9 は値を作った配置を追わないので、verifier を通った IR でも、`tobj` に入ったヒープの物体を `unbox` しうる。インタプリタは Repr を読まずに、この誤りを見つけられる
- `RunStats` に変換の回数は足さない (4節)
- 本当の箱 (`Payload::Boxed`) を確保する案は採らない。`rc_*` の回数と scaling テストが変わり、`children`、`take_or_copy`、`kind_name` に場合が増える。box の変数の RC の釣り合いは、所有の検査がすでに静的に確かめている (box の変数は所有した `tobj` で、`return` の時点で所有は残らない)。ネイティブ化を始めるときに、ネイティブに近い実行の形として見直す (ロードマップのネイティブ化)

### パスの覗き穴

- box の挿入は、変換を作る代わりに、定義をさかのぼって元の値を渡す
  - 箱を要するスカラーの変数 v を `tobj` の位置へ渡すとき、v が `let v = unbox w` で定義されていれば、`box` を作らずに w を渡す
  - `tobj` の変数 v を Repr が s のスカラーの位置へ渡すとき、v が `let v = box w` で定義され、w の Repr が s なら、`unbox` を作らずに w を渡す
- 表は、パスが作る定義 (受け直しの `unbox` と `box`、その場で一様にした関数の入口の `unbox`) からだけ作る。w は v の定義より前で定義されるので、v の使いをすべて支配し、R6 を保つ
- 使う所で作った変換は、同じ変数のほかの使いで使い回さない。使う所は、ほかの使いを支配しないためである
- 覗き穴が要る理由: これがないと、恒等に近い一様な関数 (`main$lambda3(m) = m`、handler の `return` の節、`cont$` の転送) が値を往復させる。`let r' = apply ..`、`let r = unbox r'`、`let b = box r`、`return b` は末尾呼び出しの形にならず、末尾呼び出しも失う。覗き穴があれば `return r'` になり、縮約が使われない `unbox` を消して `tail` を作る
- Perceus から見ると、w の寿命が延びるだけである。w はほかの RC の対象の変数と同じく、呼び出しをまたぐときに退避される

## 3. パスの順と末尾呼び出し

### パスの順

- パスは translate、box の挿入 (`boxing`)、縮約、Perceus の順に流す
- verifier は、translate の後に変換の段 (`verify_translated`)、box の挿入と縮約の後に範囲の段 (`verify_scopes`)、Perceus の後に所有の段 (`verify`) をかける。今と同じくデバッグビルドだけでかけ、誤りはパスの名前を付けて報告する
- `Pass` は `Translate`、`Boxing`、`Contract`、`Perceus` の4つになる。`Boxing` の名前は `boxing` である。`lower_until(.., Pass::Boxing)` は box の挿入の直後で止まるので、テストは縮約の前の IR を見られる
- spec のパスの表は次のようになる

  | パス | 受け取る IR | 渡す IR |
  |---|---|---|
  | 変換 (`translate`) | 誤りのない型付き HIR | ブロックの列。末尾呼び出し、`box` と `unbox`、RC の命令、`saved` はない |
  | box の挿入 (`boxing`) | 末尾呼び出し、`box` と `unbox`、RC の命令のない IR | 位置の規則を満たす IR。一様にした関数と `f$boxed` を含む |
  | 縮約 (`contract`) | RC の命令のない IR | 使われない純粋な `let` を消し、末尾呼び出しを作った IR |
  | Perceus | RC の命令のない IR | `dup` / `decref` / `release` と `saved` が入った IR |

- box の挿入を縮約の前に置く理由: パスは変換を位置ごとに素朴に入れればよく、使われなくなった `box` と `unbox` は、縮約がほかの使われない純粋な `let` と一緒に消す
- box の挿入は型を読まない。読むのは、呼ばれる関数のシグネチャ、内部の印、配置の表、extern の行、`perform` の `resumable` だけである
- translate が変換を入れる案は採らない。translate は関数ごとに働くので、値の参照で決まる一様化と T3 のような、プログラム全体で決まる ABI を決められない
- 後のパスの置き場所: 呼び出しを作るか行き先を変えるパス (インライン化、evidence passing など) は、translate と box の挿入の間に置き、`tail` も `box` も `unbox` も出さない。box の挿入がプログラム全体の ABI (T3、一様化、`f$boxed`) と変換を1か所で決め、縮約が末尾呼び出しを作るためである。box の挿入より後に置くパスは、位置の規則を保たなければならない。evidence passing は操作の宣言した Repr をエフェクトの表に足し、box の挿入は型を読まずにそれを読む。ロードマップの evidence passing とインライン化の項目に書く

### 末尾呼び出し

- 末尾呼び出しは縮約だけが作る。縮約は、使われない純粋な `let` を消した後で、すべてのブロックに末尾呼び出しの規則を当てる。`let x = <呼び出し>` の直後の終端が `return x` で、呼び出しの結果と呼び出し元の `ret` が互換なら、2つを `tail` にする。結果は、直接の呼び出しなら呼ばれる関数の `ret`、ほかは `tobj` で、`never` の `perform` はどの `ret` とも互換とする。対象の呼び出しの種類は今と同じである
  - 互換の条件は、互換が推移的でないために要る。x を通すと `unit` から `tobj`、`tobj` から `obj` とつながる IR でも、`tail` にすると `unit` と `obj` を直接比べることになる。translate と box の挿入の出力では、この条件はいつも成り立つ
- `FnBuilder::finish` は、`return` だけを持つ合流のブロックへの `jump` を `return` に置き換える転送を続け、`tail_call` は呼ばない。translate の出力では `jump` の実引数と行き先の引数の Repr が同じなので、転送は Repr に関わらない
- `tail_call` は縮約の中だけの関数にし、`eml_core_ir` から公開しない。使うのが縮約だけになるためである
- 縮約は、文を消したかに関わらずすべてのブロックを1回見るので、時間はブロックの数に比例する。デバッグビルドで2回目のパスが何も消さないことを確かめる検査は残す
- 縮約が `tail` にする呼び出しの結果は、上の条件により呼び出し元の `ret` と互換で、範囲の段の verifier が確かめる。T3 とパスが、互換でない末尾の位置の呼び出しの後に変換を入れているので、translate と box の挿入の出力でこの条件のために `tail` にならない呼び出しはない
- 末尾呼び出しの降格は、この段では入れない。末尾呼び出しを変換を入れた後に作るので、Repr のための降格は要らない。所有の都合の降格は、今のロードマップどおり借用パラメータと一緒に入れ、輪の上の `tail` には当てない (1節)。spec の「Perceus が行ってよい」の文と、終端をその場で編集する形の文は、「借用パラメータを入れるときに、輪の上にない末尾呼び出しにだけ降格を足す」に直す
- 転送が box の挿入より前にあるので、`return` だけの合流のブロックへ入る枝が、それぞれ `return` で `box` する。合流に1回で済んでいた `box` が枝の数だけになるが、コードの大きさの違いだけなので受け入れる

### box の挿入の手順

プログラム全体を1回で扱う。手順は次の順である。

1. 分類: 各関数について、値として参照されるか (どこかの `&f`、`closure f` の対象) と、直接呼ばれるか (`call f`) を記録する。入力に `tail`、`box`、`unbox`、RC の命令はない (変換の段が拒む)
2. T3: 関数の数と末尾の辺の数に比例する時間で、`ret` を上げる
   - 各関数の末尾の位置の呼び出しを集める。直接の呼び出し f → g は、g の逆の表に f を入れる。`never` の `perform` は集めない
   - `ret` が箱を要するスカラーで、結果が互換でない末尾の位置の呼び出しを持つ関数を上げ、作業の列に積む
   - 列から g を取り出し、逆の表の各 f について、f の `ret` が箱を要するスカラーなら上げて積む。g の `ret` は `tobj` になったので、f の `ret` とは互換でない
   - `ret` はスカラーから `tobj` へ1回だけ動くので、各関数は多くとも1回積まれ、各辺は1回だけ見る。試作の「変わらなくなるまで全体を繰り返す」形は2乗の時間になり、関数の鎖で N = 3,000 で 0.28 秒、N = 6,000 で 1.05 秒かかった
   - 一様化より前に置く理由: T3 が `ret` を `tobj` にして一様になった関数に、要らない `f$boxed` を足さないためである。T3 の結果は一様化に左右されない。その場で一様にする関数も `f$boxed` も直接は呼ばれないので、T3 が見る直接の末尾の辺を変えないからである
3. 一様化: 値として参照され、T3 の後でも一様でない関数のうち、直接呼ばれない内部の関数はその場で一様にし、ほかの関数には `f$boxed` を足す (1節)。`f$boxed` は、関数の表の末尾に、元の関数の番号の順に足す。名前は元の関数の名前 (修飾を含む) に `$boxed` を付けたものである。その後、すべての関数の `&f` と `closure f` を `f$boxed` に向け直す
4. 変換: 関数ごとに、ブロックを番号の順に、文を前から見て、各アトムを位置の Repr にする (1節の表)。extern の引数と結果は正確で、translate の出力ですでに合うので、何も入れない。`jump` と `return` も translate の出力では Repr が同じなので、変換が要るのは `ret` を変えた関数の `return` だけである
   - 互換でないアトムは、箱を要するスカラーから `tobj` へは `let b = box a` を、`tobj` から箱を要するスカラーへは `let v = unbox a` を、使う文の前に置いて渡す。変換は箱を要するスカラーと `tobj` の間だけで、`obj` とは行き来しない。2節の覗き穴を先に試す
   - 束縛の位置 (直接の呼び出し、`apply`、`perform`、`resume`、`handle` の結果、`unpack` と case のフィールド) で、受け取る Repr と変数の Repr が互換でなければ、受け取る Repr の新しい変数で受ける。元の変数は、その直後で `unbox` か `box` で定義する。case のフィールドなら、その定義を行き先のブロックの先頭に置く。R3 から行き先へ入る辺はその `switch` の1本だけなので、定義は使いをすべて支配する。`never` の `perform` の束縛は受け直さない
   - 使う所の変数は書き換えないので、R5 (1回の定義) と R6 (支配) はそのまま成り立つ
   - パスが作る変数の名前: 新しい変数は、元の変数の名前に、変数の表の末尾の番号を付けたものにする (`x.5: tobj`)。`Int` の定数を `box` した変数は `b`、`f$boxed` の結果の変数は `t` とする。その場で一様にした関数の新しい引数と `f$boxed` の引数も、元の引数の名前を使う。スナップショットはこの名前で決まる
5. 型から起きない変換に当たったら、パスはパニックする。箱を要するスカラーと `obj` の間 (どちら向きも)、違う Repr のスカラーどうし、`unit` と箱を要するスカラーの間である。translate の出力では起きないので、内部の誤りである。試作では UI とテストのどこでも起きなかった

- パスはブロックと文のループだけで、IR の大きさに比例して再帰しない

### REPL で走らせ直すもの

- 後の定義によって変わる ABI は、`f$boxed` を足すかどうかだけである。`f$boxed` は足すだけで、元の関数もそれまでの参照も変えない。REPL は、ある関数への最初の値の参照が現れた入力で `f$boxed` を足せばよい
- 内部の関数の一様化は、それを作った定義の中で決まる
- T3 は、関数の本体と、それが末尾の位置で直接呼ぶ関数の `ret` だけで決まる。呼ぶ関数は前の入力か同じ再帰の組で定義されるので、新しい定義が古い定義の `ret` を変えることはない
- そのため、定義ごとに Core IR を保存する REPL でも、box の挿入をプログラム全体に走らせ直さずに済む。ロードマップの REPL の項目に書く

### translate のテストへの影響

- translate は `tail` を出さなくなる。`Pass::Translate` のスナップショットの `tail X` の行は、`let t.N: r = X` と `return t.N` の2行になる。`translate.rs` に残る 36 のテストの 67 行である。下で移す3つのテストの6行は、`Pass::Contract` で `tail` のまま残る
- 末尾呼び出しそのものを確かめる次の4つのテストは、同じ名前のまま `contract.rs` に移し、`Pass::Contract` のスナップショットにする。移した直後 (4節の計画の手順2) は、今の `Pass::Translate` のテキストと同じである
  - `calls_in_tail_position_are_tail_calls`
  - `a_returned_if_value_becomes_tail_calls_in_each_arm`
  - `a_returned_match_value_becomes_tail_calls_in_each_arm`
  - `returning_a_field_of_a_call_result_is_not_a_tail_call` (`Pass::Translate` では `tail` がいつもないので、この主張は縮約の後でしか意味を持たない)
- 転送のテスト (`a_chain_of_returned_continuations_folds_in_one_pass`) と、合流のブロックを作らないことのテスト (`a_tail_if_returns_from_each_arm`、`tail_and_non_tail_matches`) は、translate の働きを確かめるので `translate.rs` に残す。前者は上の `tail` の行の書き換えだけを受ける
- ほかの `Pass::Translate` のスナップショットは、内部の印 (4節の期待値の変更 F) のほかは変わらない。translate の出力は box の挿入の前のものだからである

## 4. verifier、見張り、段の形、テスト

### verifier の段

| 段 | 関数 | かける所 | 確かめること |
|---|---|---|---|
| 変換の段 | `verify_translated` | translate の後 | R1〜R5、R6 の支配、`jump` と `return` の互換、今の R8 の残り (`unpack`、`extern`)、今の R9 (宣言した Repr が `tobj` のフィールドを除く)、引き継ぐ検査。RC の命令、空でない `saved`、`tail`、`box`、`unbox` がないこと |
| 範囲の段 | `verify_scopes` | box の挿入の後、縮約の後 | 変換の段から `tail`、`box`、`unbox` の拒否を除き、`box` と `unbox` の規則と境界の検査を足す |
| 所有の段 | `verify` | Perceus の後 | 範囲の段に加えて R6 と R7 の所有。`unbox` を読む使いとして確かめる |

- 境界の検査は、この段で足す R8 と R9 の検査のうち、`jump` と `return` と `box` と `unbox` の規則を除くすべてである。直接の呼び出しの引数と結果、`closure` の引数、関数の値の対象が一様であること、一様なオペランドと結果、`tail` の結果、`tobj` のフィールドである
- 関数の値の対象が一様かは、verifier を始めるときに関数ごとに1回求めて表にし、`&g` と `closure g` では表を引くだけにする。参照ごとに引数をたどると、参照の数と引数の数の積の時間になり、spec の「verifier は線形のままである」に反する
- 段は直線に並ぶので、境界の検査だけを切り替える別の旗は持たない。`Level` は `Translated`、`Scopes`、`Ownership` の3つである
- `eml_core_ir` は `verify_translated` と `boxing` を公開する。`boxing` を公開するのは、`contract` と `perceus` と同じく、IR のテキストからのテスト (T3 の時間) のためである。テストの補助に、`contract_text` と並べて `boxing_text` を足す
- 変換の段が `tail`、`box`、`unbox` を拒む理由: box の挿入は末尾呼び出しと変換のない入力を前提にし、T3 は `let` と `return` の形から末尾の位置を見る。覗き穴の表も、パスが自分で作った定義からだけ作る。Perceus より前に `release` を拒むのと同じく、パスの入力の約束を verifier が確かめる
- spec の R8 は次のように書き直す。`extern` の引数と結果は表の行と同じである。`unpack` の値は `obj` の変数である。`jump` の実引数と行き先の引数、`return` の値と `ret`、直接の呼び出しと `apply`、`perform`、`resume`、`handle` の引数と結果、`closure` の引数、`tail` の結果は、1節の互換の関係で比べ、定数は互換の位置の当てはめで比べる。`never` の操作の `perform` の結果と、その束縛の使いは比べない。関数の値として使う関数は一様である。`box` と `unbox` は2節の規則に従う
- spec の R9 には、宣言した Repr が `tobj` のフィールドでは、`con` の引数と case と `unpack` の束縛が `tobj` と互換であることを足す

### 検査の順

- 今と同じく、それぞれの命令で、形の検査、R8 と R9 (境界の検査を含む)、範囲と所有の検査 (R5、R6、R7) の順に行う。今ある文言は変えない
- 1つの呼び出しの中では、引数の数、引数の Repr (左から)、結果を束縛する変数の Repr の順に比べ、その後で範囲と所有を確かめる。今の extern の検査と同じ順である。`tail` では、引数の Repr の後で結果と呼び出し元の `ret` を比べる
- `closure g(..)` では、引数の数の後で g が一様かを確かめ、その後で引数の Repr を比べる
- 範囲の段と所有の段では、`&g` は、その位置の定数の当てはめの中で、`tobj` に収まるかを確かめた後に g が一様かを確かめる
- 見える影響が1つある。`closure` の対象が一様でない誤りは、その引数の範囲の誤りより先に出る

### 文言

どの文言の後にも、今と同じく `` in `f` `` が付く。下では付けた形で書く。`handle` の命令は、今の文言 (``a handler of `State` has clauses for …``、``the `return` clause needs 2 parameters …``) に合わせて handler と呼ぶ。

範囲の段と所有の段 (境界の検査)

- 直接の呼び出しの引数: ``argument 0 of `g` is `x.1` (int), but the function takes tobj in `f` ``、``argument 1 of `g` is 5, but the function takes tobj in `f` ``
- 直接の呼び出しの結果: `` `t.0` (obj) is bound to `k`, which returns int in `f` ``
- `closure` の引数: ``argument 0 of a closure of `g` is `n.1` (int), but the function takes tobj in `f` ``
- 関数の値の対象: `` `g` is used as a function value, but its parameter 0 is int in `f` ``、`` `g` is used as a function value, but it returns int in `f` ``
- `apply` のオペランド: ``the callee of an apply is `n.1` (int), but an apply takes tobj in `f` ``、``argument 0 of an apply is `n.1` (int), but an apply takes tobj in `f` ``
- `perform` のオペランド: ``argument 0 of a perform of `Ask.ask` is 1, but a perform takes tobj in `f` ``
- `resume` のオペランド: ``the continuation of a resume is `n.1` (int), but a resume takes tobj in `f` ``、``the value of a resume is 1, but a resume takes tobj in `f` ``、``the state of a resume is `s.2` (int), but a resume takes tobj in `f` ``
- `handle` のオペランド: ``the initial state of a handler of `State` is 0, but a handler takes tobj in `f` ``、``the body of a handler of `Ask` is `b.1` (int), but a handler takes tobj in `f` ``、``the clause for `ask` of a handler of `Ask` is `c.2` (int), but a handler takes tobj in `f` ``、``the `return` clause of a handler of `Ask` is `r.3` (int), but a handler takes tobj in `f` ``
- 一様な結果: `` `t.0` (int) is bound to an apply, which returns tobj in `f` ``。ほかは ``a perform of `Ask.ask` ``、``a resume``、``a handler of `Ask` `` を同じ形で使う
- `tail` の結果: ``a tail call to `g` returns int, but this function returns tobj in `f` ``、``a tail apply returns tobj, but this function returns int in `f` ``。ほかは ``a tail perform of `Ask.ask` ``、``a tail resume``、``a tail handler of `Ask` `` を同じ形で使う。呼び出し元を "this function" と書き、呼ばれる側を指す "the function" と分ける
- `tobj` のフィールド: 今の R9 の文言を `tobj` に広げる。``field 0 of `Option` #1 is `x.3` (int), but the layout has tobj in `f` ``、``argument 0 of a con of `Option` #1 is 5, but the layout has tobj in `f` ``

3つの段すべて

- `jump` と `return`: 今の文言のまま、互換で比べる。定数では ``5 is returned from a function that returns tobj in `f` ``、``() is returned from a function that returns obj in `f` ``。今の変数の文言 `` `x.1` (int) is returned from a function that returns obj in `f` `` と同じ形である

範囲の段と所有の段 (`box` と `unbox`)

- `box` のオペランド: `` `u.1` (unit) is boxed, but only int and enum values and Int constants can be in `f` ``。定数は ``() is boxed, but only int and enum values and Int constants can be in `f` `` (`#1`、`&g` も同じ形)。今の `` `p.0` (int) is unpacked, but only obj can be `` と同じ形である。`int and enum` の部分は、箱を要するスカラーの定義から作る
- `box` の束縛: `` `b.2` (int) is bound to a box, which is tobj in `f` ``
- `unbox` のオペランド: `` `p.0` (obj) is unboxed, but only tobj can be in `f` ``、``5 is unboxed, but only tobj can be in `f` ``
- `unbox` の束縛: `` `n.2` (tobj) is bound to an unbox, which gives int or enum in `f` ``。`int or enum` の部分も同じ定義から作る

変換の段だけ

- `tail`: ``a tail call is formed before contract in `f` ``。`call` のほかは ``a tail apply``、``a tail perform``、``a tail resume``、``a tail handler`` を同じ形で使う。今の ``a call saves [..] before Perceus`` と同じく文にする
- `box` と `unbox`: `` `n.1` is boxed before the boxing pass in `f` ``、``5 is boxed before the boxing pass in `f` ``、`` `b.2` is unboxed before the boxing pass in `f` ``。今の `` `d.0` is released with its fields before Perceus `` と同じ形である

所有の段だけ

- `unbox` の読み: 今の読む使いの一覧に `unboxed` を足す。`` `x.1` is unboxed after its owner `d.0` was given up in `f` ``、`` `x.1` is unboxed after it was moved in `f` ``

### 実行の見張り

- `RunStats` に5つ目の回数 `peak_objects` を足す。同時に生きていたヒープの物体の数の最大で、フレームと不死のリテラルを含む
  - ヒープは空いたスロットを先に使い、空きがないときだけスロットを足す。そのため `Heap::slots.len()` がそのまま最大になり、数える手間はない。`Heap::peak_objects` として返す
  - フレームの伸びを見られる回数はこれだけである。末尾呼び出しを失うと、反復の数に比例して増える
  - `peak_objects` は仕事の回数でなく、ヒープの物体の数の最大である。spec と文書の `RunStats` の定義の文 (「実行の仕事の回数」) を、「実行の仕事の回数と、同時に生きていたヒープの物体の数の最大」に直す
- 歩数 (`steps`) と変換の回数 (`conversions`) は足さない。歩数は IR の形を変えるたびに変わり、上限を決めにくい。末尾呼び出しを失ったことは `peak_objects` で、2乗の時間は今の4つの回数で見えるので、歩数で見張るものが残らない。`box` の増減は、`Pass::Boxing` と `Pass::Contract` のスナップショットで静的に固定するほうが確かである (「単相の `Int` のループには `box` がない」など)
- `eml_interp/tests/scaling.rs` に、ループの形ごとに `peak_objects` を n = 1000 と n = 2000 で比べるテストを足す。ソースはテストの中で作る。どれも今の main で通ることを確かめた

  | 形 | ソースの要点 | 確かめること |
  |---|---|---|
  | `Int` を返す関数の値を通るループ | `loop f n = if n == 0 then 0 else f (n - 1)`、`go n = loop go n` | 2つの n で同じ |
  | `Bool` を返す関数の値を通るループ | `is_even odd n = if n == 0 then True else odd (n - 1)`、`is_odd n = if n == 0 then False else is_even is_odd (n - 1)` | 2つの n で同じ |
  | `Unit` を返す関数の値を通るループ | `tick f n = if n == 0 then println "done" else f (n - 1)`、`run n = tick run n` | 2つの n で同じ |
  | 呼び出しと結果の間に使われない `let` があるループ | `loop f n = if n == 0 then 0 else let r = f (n - 1) in let s = "unused" in r`、`go n = loop go n` | 2つの n で同じ |
  | 節が末尾で再開する操作のループ | `sum_asks n acc = if n == 0 then acc else sum_asks (n - 1) (acc + ask ())` を `\| ask () k -> k 2` の handler で包む | 2つの n で同じ |
  | 直接の自己末尾呼び出し | `loop n acc = if n == 0 then acc else loop (n - 1) (acc + 1)` | 2つの n で同じ |
  | ラムダからラムダへの末尾の `apply` | `count_down n k = if n == 0 then k 0 + k 0 else count_down (n - 1) (fn m -> k (m + 1))` | n = 2000 の値が n = 1000 の値 + 1000 以下 (反復ごとにクロージャが1つで、フレームは積まない)。クロージャの鎖を2回使うのは、戻る間も鎖を共有にしておくためである。一意なクロージャは `apply` で手放されるので、そのスロットを積んだフレームが使い回し、末尾の `apply` を失っても `peak_objects` が増えない |

- T3 の時間: `eml_core_ir/tests/boxing.rs` で、N = 30,000 の関数の鎖を IR のテキストで作り、`boxing_text` で公開した `boxing` を当てる。`f_i` は `f_{i+1}` を末尾の位置で呼び、最後の関数は `&k` への `apply` を末尾の位置で呼ぶ。呼ばれる側の番号を大きくして、全体を繰り返す不動点が2乗の時間になる並びにする。`boxing` の時間だけを測り、debug ビルドで 5 秒未満であることと、鎖のすべての `ret` が `tobj` になることを確かめる。試作の2乗の形では N = 6,000 で 1.05 秒だったので、N = 30,000 では約 26 秒になる。verifier の大きな IR のテストと同じく、`#[ignore]` を付けずにふだんの `cargo test` で流す

### 段の形と計画

1つの spec と1つの計画にし、次の順の手順に分ける。どの手順の後も、ワークスペースのテストはすべて通る。

1. `RunStats::peak_objects` と見張りのテスト
   - `peak_objects` を足し、上の表の7つの形のテストを足す。今の main で通るので、後の手順が末尾呼び出しを失えば、このテストが落ちる
   - `a_program_without_operations_or_strings_does_no_counted_work` を書き換える (期待値の変更)
2. 末尾呼び出しを縮約だけで作る
   - `finish` から `tail_call` を外し、縮約がすべてのブロックに当てる。`tail_call` を公開しない
   - translate の `tail` の行を書き換え、4つのテストを `contract.rs` に移す。縮約は translate の直後に走るので、縮約と Perceus の出力と UI は1文字も変わらない
3. `box` と `unbox` の命令と、内部の関数の印
   - IR、テキスト、`pretty`、`Rhs::for_each_consumed`、縮約の `pure`、Perceus の `unbox` の規則、インタプリタ、verifier のオペランドと束縛と読む使いの規則
   - `CoreFn` の内部の印、translate が印を付けること、テキストの `internal fn`
   - テストは手書きの IR だけで書く。translate はまだ `box` と `unbox` を出さない
4. box の挿入のパス
   - 分類、T3 の作業の列、一様化と `f$boxed`、変換、覗き穴、`never` の `perform`、`Pass::Boxing`、`boxing` の公開、verifier の3つの段、境界の検査、`jump` と `return` の互換、変換の段の `tail` と `box` と `unbox` の拒否、縮約が `tail` を作るときの互換の条件
   - 手書きの IR の書き換え (成否の変更と期待値の変更) と、`Pass` の網羅の `match` の追随 (機械的な追随)
   - box の挿入のテストと T3 の時間のテストを足す
5. 文書 (下の「更新する文書」)

1つの計画にする理由: どの部分も1つの ABI の決定に属する。手順2は単独で入れても害はないが、box の挿入がなければ入れる理由がない。境界の検査は box の挿入より前には入れられない (UI のプログラムの translate の出力が、数百か所で反する)。試作の差分は、テストの書き換えを含めて 2261 行だった。

### テストの変更

[テスト戦略](../../implementation/testing.md) の「テストの変更の運用」の種類で書く。範囲は試作で確かめた。ただし、`eml_interp` の8本の書き換えのうち7本と、内部の印の書き換え (F) は試作になく、手順3と手順4で確かめる。

成否の変更

- `crates/eml_core_ir/tests/verify.rs` の `the_result_of_a_call_is_not_compared_with_the_callee` は、通っていた IR が拒まれるようになる。名前を `the_result_of_a_call_is_compared_with_the_callee` にし、`` `t.0` (obj) is bound to `k`, which returns int in `f` `` を期待する。直接の呼び出しの結果を `ret` と比べるのがこの段の決定だからである (手順4)

期待値の変更 (範囲)

- A. `Pass::Translate` のスナップショットの `tail` の行 (手順2)。`crates/eml_core_ir/tests/translate.rs` に残る 36 のテストの 67 行が、`let t.N: r = <呼び出し>` と `return t.N` になる。末尾呼び出しを translate でなく縮約が作るためである
- B. 末尾呼び出しのテストの移動 (手順2)。上の4つのテストを、同じ名前で `crates/eml_core_ir/tests/contract.rs` の `Pass::Contract` のスナップショットに移す。削除ではない。手順4では、このうち3つ (`calls_in_tail_position_are_tail_calls`、`a_returned_match_value_becomes_tail_calls_in_each_arm`、`returning_a_field_of_a_call_result_is_not_a_tail_call`) が変換を含むテキストになる。総称的なフィールドの受け直しと、T3 で `ret` が `tobj` になって `tail apply` になる `call_twice` である
- C. 縮約の末尾呼び出しの規則の変更 (手順2)。`contract.rs` の `a_call_left_at_the_end_of_a_changed_block_becomes_a_tail_call` は、`h` が `tail call g(x.0)` になる。名前を `a_call_returned_at_the_end_of_an_unchanged_block_becomes_a_tail_call` にし、「translate が当て終えている」というコメントの前提を書き直す
- D. 一様な位置か `tobj` のフィールドにスカラーを渡す Core IR (手書きの IR と、ソースからの `Pass::Contract` と `Pass::Perceus` のスナップショットのうち、box の挿入が変換か `f$boxed` を入れるもの) (手順4)。IR を位置の規則に合わせて書き直す (`int` の引数と結果を `tobj` にする、`box` と `unbox` を置く、組の代わりに `int` のフィールドを持つ配置を使う)。そうしないと、テストの目的と関係のない境界の検査で落ちるためである。試作で見つかったのは次のテストである
  - `crates/eml_core_ir/tests/contract.rs`: `unused_bindings_that_cannot_fail_are_removed`、`bindings_used_only_by_removed_bindings_go_in_the_same_pass`、`every_kind_of_call_returned_at_the_end_becomes_a_tail_call` (値として使う `body` と `clause` の `return ()` は、`tobj` を返す関数でも互換の位置なのでそのまま書ける)
  - `crates/eml_core_ir/tests/perceus.rs`: `a_live_unpacked_value_dups_the_fields_used_later` (手書き)、`a_default_target_owns_the_scrutinee_without_a_dup` (ソースからのスナップショット。`List Int` のフィールドが `tobj` で受け直される)
  - `crates/eml_core_ir/tests/verify.rs`: `handlers_operations_and_resume_are_calls`、`a_handler_has_a_clause_for_each_operation` (この2つは補助の `handler_program`)、`a_clause_of_a_never_operation_receives_the_arguments_and_the_state`、`a_return_clause_receives_the_value_and_the_state_after_its_captures`、`a_clause_outside_its_scope_is_rejected_before_its_parameters_are_counted`、`a_borrowed_field_cannot_be_consumed` (`apply x.2(1)` を `apply x.2(())` に)、`a_release_that_keeps_a_field_that_is_not_rc_is_rejected` (組の代わりに `layout P { P(int, tobj) }`)
  - `crates/eml_interp/tests/closures.rs`: `a_partial_application_waits_for_the_rest_of_the_arguments`、`a_returned_function_can_still_wait_for_more_arguments`、`extra_arguments_are_applied_to_the_returned_function`、`a_shared_closure_keeps_its_captured_values`、`a_function_value_is_applied_like_a_closure_without_arguments`、`a_function_value_needs_no_reference_counting`、`an_inner_handle_returns_through_each_resumption_of_an_outer_multi_operation`
  - `crates/eml_interp/tests/data.rs`: `an_unpack_reads_the_fields_without_taking_the_box`
- E. `RunStats` の回数の追加 (手順1)。`crates/eml_interp/tests/scaling.rs` の `a_program_without_operations_or_strings_does_no_counted_work` は、`RunStats::default()` と比べる代わりに、4つの仕事の回数が 0 で、`peak_objects` が 1 (`Frame::Root` のフレーム) であることを確かめる。`peak_objects` は仕事の回数でなく、`main () = ()` でも 0 にならないためである
- F. 内部の関数の印 (手順3)。ソースから作る Core IR のスナップショットで、内部の関数の行が `fn` から `internal fn` になる。`translate.rs`、`contract.rs`、`perceus.rs` のスナップショットが対象である。印をテキストに書かないと、`pretty` と `parse` の往復で印が落ちるためである

機械的な追随

- `crates/eml_core_ir/tests/common/mod.rs` の `read_back` の `match`: `Pass::Translate` は `verify_translated`、`Pass::Boxing` と `Pass::Contract` は `verify_scopes` で確かめる。`boxing_text` を足す
- `contract.rs` の `every_kind_of_call_returned_at_the_end_becomes_a_tail_call` は、`tail_call` を直接呼ぶ代わりに `contract_text` を通す (手順2。出力は変わらない)。`tail_call` の `use` を消す
- `contract.rs` の冒頭のモジュールのコメント (「消したブロックの末尾に末尾呼び出しの規則をもう一度当てる」) を、すべてのブロックに当てる形に直す (コメントだけの変更)
- `crates/eml_core_ir/tests/main.rs` に `mod boxing;` を足す

### 足すテスト

- `crates/eml_core_ir/tests/boxing.rs` (新しいファイル)。ソースから `Pass::Boxing` で見る
  - 値としてだけ使う内部の関数 (ラムダ) をその場で一様にする
  - 値としてだけ使うトップレベルの関数にも `f$boxed` を足し、関数そのものは形を変えない
  - 直接も呼ぶ関数に `f$boxed` を足し、値の参照を向け直す。`f` の `ret` が一様なら、`f$boxed` の本体は `Pass::Contract` で `tail call` になる
  - T3 で `ret` が `tobj` になって一様になった関数には `f$boxed` を足さない
  - case のフィールドを行き先の先頭で、`unpack` のフィールドをその直後で受け直す
  - `Int` の定数を `box` し、`()`、`#N`、`&f` はそのまま渡す
  - 覗き穴の両向き (恒等のラムダの `return`、受け直した値をもう一度参照の位置へ渡す形)
  - `unit` の値を命令なしで `tobj` の位置へ渡す
  - 単相の `Int` のループには `box` がない
  - `never` の操作で失敗する関数は、スカラーの `ret` を保つ (`effect_abort.em` の `check_positive` の形)
  - T3 が鎖をたどって `ret` を上げる。使われない純粋な `let` を挟む末尾の位置でも上げる。輪の上にない残りの形 (`$handleN` の本体が `Int` を返す関数を末尾の位置で呼ぶ) は、`Pass::Contract` で普通の呼び出しのまま残る
  - T3 の時間 (上の「実行の見張り」)
- `verify.rs`: 上の文言ごとに1つ。`obj` の値を `tobj` のブロックの引数へ `jump` で渡せること、`tobj` を返す関数から `return ()` できること。変換の段は一様でない `g` の `&g` を `jump` の実引数に許し、範囲の段は拒むこと。`never` の `perform` の束縛はどの Repr でもよいこと
- `text.rs`: `box` (変数と `Int` の定数) と `unbox` の往復。`internal fn` の往復
- `perceus.rs`: `unbox` の後で死ぬオペランドに `decref` を置くこと、生きているオペランドには置かないこと、case のフィールドを `unbox` する形
- `contract.rs`: 移した4つのテストと、名前を変えたテスト (C)。呼び出しの結果と呼び出し元の `ret` が互換でない形 (`unit` を返す関数の結果を `tobj` の変数で受け、`obj` を返す関数がそれを返す) は `tail` にしないこと
- `eml_interp/tests/data.rs`: verifier を通った IR で、`tobj` に入ったヒープの物体を `unbox` すると内部の誤りになること (R9 の限界を固定する)。`run_core_unverified` で、`obj` の変数を `box` すると内部の誤りになること
- `eml_interp/tests/scaling.rs`: 上の7つの形

### 対象外

それぞれ、どこに書くかを添える。

- 操作の宣言した Repr: ロードマップの「その後の項目」の「処理系」の evidence passing の項目に、1節の内容と UI での効果 (動的に 220,123 回) を書く。evidence passing を box の挿入より前に置き、操作の Repr をエフェクトの表に足すことも一緒に書く (3節の「パスの順」)
- `Int` と `enum` を命令なしで `tobj` に置くこと、`Int` の幅、`box 5` の定数を共有の不死の物体にすること (Lean の `_boxed_const`)、インタプリタで本当の箱を確保する形、型変数の位置の `Int` (`List a` のフィールドで 302,149 回、CPS のラムダで 200,125 回) を特殊化で消すこと: ロードマップのネイティブ化の項目にある「多相な位置での `Int` と `Float` の表現」に、`box` と `unbox` がその決定を入れる場所であることと一緒に書く
- `unbox` の後の `decref` と、型変数のフィールドで増える `release` (UI で 50 から 65): ロードマップの「Perceus の最適化」の、フィールドを死ぬ所で手放す形の項目に書く
- 所有の都合による `TailCall` の降格: ロードマップの借用パラメータの約束のまま残し、輪の上の `tail` に当てない約束を足す (1節)
- インライン化を box の挿入より前に置くこと: ロードマップの jump threading とインライン化の項目に書く (3節の「パスの順」)
- REPL でのプログラム全体の ABI: ロードマップの REPL の項目に、後の定義で変わるのは `f$boxed` を足すことだけで、走らせ直しは要らないことを書く (3節)
- 文を持つ合流のブロックを通る末尾の値: 縮約が使われない `let` を消した後で、`return` だけになった合流のブロックへの転送をやり直せば、保証の範囲を広げられる。今の main と同じ限界なので、この段では入れず、`status.md` の「深さと性能」に書く
- 戻らない extern: ロードマップの S4 の `exit` の項目に、`never` の `perform` と同じ扱いを決めることを書く
- 採らない案 (書かない): 呼ばれる側も上げる T3、輪の上だけの T3、種類で決める一様化、トップレベルの関数のその場の一様化、`jump` と `return` を正確にすること、handler ごとの答えと状態、消費する `unbox`、`unbox` の `Stmt`、`steps` と `conversions` の回数

### 確認の手順

- `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check`
- 既定でない feature の組み合わせ: `cargo clippy -p eml_cli --no-default-features` と `--features types`、`--features core`。`eml_test_support` の同じ組み合わせと `--features hir`
- `cargo test -p eml_cli --test integration citations`
- `nix build`

### 更新する文書

段の終わりに直す。ただし、コードのコメントが引く `docs/spec/core-ir.md` の新しい節の見出し (「位置の規則」と「box の挿入」) は、そのコメントと同じコミットで足す (citations のテストがあるため)。

- `docs/spec/core-ir.md`
  - 命令の表と読む使い (`box` と `unbox`、`unbox` は読む)
  - `tail` の文 (縮約が作る、互換の条件、降格は借用パラメータと一緒に輪の上にないものにだけ足す)
  - R8 と R9 (上の書き直し)
  - 「値の表現」 (`:88` の S3b-2c-2 の文を消し、一様な位置、互換の関係、`unit` の規則、箱を要するスカラーの `box`、バックエンドの約束を書く) と、新しい節「位置の規則」 (1節の表、一様な関数、内部の関数、`f$boxed`、捕獲、エフェクトの位置、`never` の操作、T3)
  - 「データの配置」の `:102` の S3b-2c-2 の文 (`tobj` のフィールドは互換で比べる。「`box` と `unbox` が要るのはスカラーと参照の間だけ」に `unit` の例外を書く)
  - 変換の規則の末尾呼び出しの段落 (縮約が作る、末尾の位置の呼び出しの IR の形での定義、保証の文、合流のブロックを通る形は保証の外)
  - パスの表、パイプラインの文、`:170` の「今あるパス」の文、新しい節「box の挿入」 (3節の手順と、後のパスの置き場所)
  - 縮約 (すべてのブロック、`box` と `unbox` は純粋)
  - Perceus (`unbox` の規則、降格の文)
  - verifier (3つの段、検査の順、文言、一様かの表)
  - インタプリタ (`box` と `unbox` は値をそのまま渡し、ヒープの物体なら内部の誤り)。`:258` の読むだけの一覧に `unbox` を足し、`:274` の位置を付けない誤りに `box` と `unbox` の内部の誤りを足す
- `docs/spec/runtime.md`: 「実行の API」の `:73` の `RunStats` の定義の文を直し、回数を5つにして `peak_objects` を書く。`:78` の「回数は時間ではなく仕事を数える」の文を、`peak_objects` に合わせて直す
- `docs/implementation/testing.md`: テキストの形 (`box`、`unbox`、`internal fn`)、`:108` と `:114` の例 (`n.2: int` を `n.2: tobj` に、`t.3: int` を `t.3: tobj` に。どちらも新しい境界の検査に反するため)、`Pass::Boxing`、`read_back` の段、`boxing_text`、性能のテストの段落 (`peak_objects` の形と T3 の時間)
- `docs/implementation/architecture.md`: パスの順と verifier の段、`finish` が末尾呼び出しを作らないこと、縮約、`boxing.rs`、`Rhs::for_each_consumed` と `unbox` の読む使い、`RunStats` のヒープが数える回数 (4つ)、`:274` の `RunStats` の定義の文
- `docs/implementation/status.md`: 「まだ比べない」の項目を消し、「深さと性能」に、輪の上にない末尾の位置の呼び出しが普通の呼び出しになることと、文を持つ合流のブロックを通る末尾の値が保証の外にあることを書く
- `docs/future/roadmap.md`: `:5` の「S3b-2c-2〜S5」、段の表と節から S3b-2c-2 を消し、S4 の前提を「なし」にする。並べた理由の S3b-2c-2 の文を直す。対象外の記録 (evidence passing、ネイティブ化、Perceus の最適化、借用パラメータ、インライン化、REPL、S4 の `exit`) を足す
- `docs/README.md` の roadmap の行 (「S3b-2c-2〜S5」を「S4〜S5」に)、[全体設計](2026-10-07-redesign-design.md) の `:109` の `eml_core_ir` の行 (末尾呼び出しを translate で作るという記述とパスの列) と `:133` の S3b の行、`CLAUDE.md` (パスの列、末尾呼び出しを作る場所、`unbox` が読む使いであること、verifier の3つの段、`RunStats` の work counters の文)
- コードのコメント: `translate/types.rs:9-11`、`verify.rs` の冒頭と `:707-708`、`contract.rs` の冒頭と `tail_call`、`translate/builder.rs` の `finish`、`perceus.rs:93-95` (降格)、`lib.rs` の `Term::TailCall` (「translate が出す」)

最後に、`grep -rn -e 'S3b-2c-2' -e 'translate が出す' -e 'tail_call' -e '実行の仕事の回数' -e 'work counters' docs CLAUDE.md crates` が、意図して残す記述だけを出すことを確かめる。

### 完了の条件

- UI テストの出力が変わらない
- `box` と `unbox` が IR にあり、box の挿入のパスが位置の規則を満たす IR を作り、範囲の段と所有の段の verifier が境界を比べる
- 末尾呼び出しは縮約だけが作り、7つの形の `peak_objects` のテストと T3 の時間のテストが通る
- 上の確認の手順がすべて通る
