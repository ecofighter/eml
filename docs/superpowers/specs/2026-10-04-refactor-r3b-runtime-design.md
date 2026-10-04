# リファクタリング R3b: ランタイムとインタプリタの設計

位置づけ: 作業用の設計文書。R3b を終えたら、残す価値のある内容を `docs/spec/core-ir.md`、`docs/spec/runtime.md`、`docs/implementation/architecture.md`、`docs/implementation/status.md` に移し、この文書は削除する。

## 目的と範囲

[status.md](../../implementation/status.md) の「R3b ランタイムとインタプリタ」の一覧を行う。リファクタリングの最後の回である。

- `Frame` を種類の enum にする。`ApplyFrame` を別のペイロードにするのをやめ (「テストを変えないために曲げた箇所」の表の1)、番兵の `IO_HANDLER` と `slots: Option` をなくす。記述子はペイロードの種類から決める
- 共有されたオブジェクトを複製する手続きをランタイムに置き、参照を `dup` せずに複製する `Clone` をなくす
- 変数のスロットごとの参照の数 (`Owned::refs`) をなくす。代わりに、呼び出しの後で使う変数を Core IR に記録し、フレームはその変数だけを退避する
- `RuntimeError` と `step` の結果に型を付ける。`eml_cli` の `RunResult` をなくす

`eml_runtime`、`eml_interp`、`eml_cli` を変え、`eml_core_ir` を追随させる。

リファクタリング全体の方針 (UI テストの出力は原則として変えない、段階3〜5の器の形は作り替えるが機能は実装しない) と、テストの変更の運用 ([testing.md](../../implementation/testing.md)) に従う。R3b は言語の観測できる振る舞いを変えない。実行時エラーの文言も変えない。UI テストの期待値は変わらない。

次のことは、ユーザーと合意済みである。

- `Owned::refs` は、呼び出しの時点で生きている変数だけをフレームに退避する形 (Perceus の標準の形) に置き換えてなくす
- フレームの種類、記述子、複製の手続き、型を付けたエラーは、下の 1〜4 の形にする
- `eml_runtime` の単体テスト `registered_descriptors_are_counted_by_name` を削除する (種類1)

## 1. フレームと記述子

`eml_runtime` のペイロードとフレームを次の形にする。

```rust
pub enum Payload {
    Str(String),
    Closure(Closure),
    Frame(Frame),
}

/// CEK 機械の継続のフレーム。継続もランタイムのオブジェクトにする (docs/spec/runtime.md)。
pub enum Frame {
    /// 呼び出し元の関数に戻る。呼び出しの後で使う変数だけを退避する。
    Return {
        function: u32,
        resume: u32,
        bind: u32,
        saved: Vec<(u32, Value)>,
        next: ObjRef,
    },
    /// 戻った関数値に、余った引数を適用する (docs/spec/core-ir.md の eval/apply)。
    Apply { args: Vec<Value>, next: ObjRef },
    /// 継続の最下部にある `IO` の組み込みの handler (docs/spec/core-ir.md)。
    Io,
}
```

- `Payload::ApplyFrame` と `ApplyFrame`、インタプリタの番兵 `IO_HANDLER = u32::MAX`、`Frame::slots` と `next` の `Option` はなくなる。継続の終わりは `Frame::Io` で表す。
- `Heap::alloc(payload)` は、ペイロードだけを受け取る。記述子はペイロードの種類から決める (`Str` は `String`、`Closure` は `Closure`、`Frame` はどの種類も `Frame`)。リークの報告の形 (`1 Frame` など) は変わらない。
- `Heap::register` をなくし、`DescId` と `Descriptor` を公開の API から外す。段階4で、ユーザーの `data` の値のペイロードが自分の記述子を持つときに、登録を足し直す。
- 解放 (`decref` の作業リスト) は、`Return` の退避した値、`Apply` の引数、どちらの `next` も子としてたどる。退避した値はそれぞれ参照を1つ所有するので、1回ずつ解放する。

## 2. 呼び出しのフレームに退避する変数

### Core IR

`Rhs::Call(Call)` を次にする。

```rust
pub enum Rhs {
    // ...
    /// 呼び出しの後で使う変数 (`saved`) を、呼び出しのフレームに退避する。
    Call { call: Call, saved: Vec<VarId> },
    // ...
}
```

- 変換は `saved` を空で作る。Perceus の後の新しいパス (`saved.rs`) が、各呼び出しの `saved` を、その呼び出しの後で使う変数で埋める。RC の対象でない変数 (`Int`、`Bool` など) も含む。`Jump` の先の join point の本体で使う変数も含む。
- 生存解析 (`liveness.rs`) は、調べる変数を引数の印で選ぶ形のまま使う。Perceus は RC の対象の変数だけを、退避のパスはすべての変数を調べる。
- 表示は、`saved` が空でなければ後ろに `[...]` を付ける (例: `let t2 = call twice(s1) [s3]`、`let t3 = apply c2(1) [c2]`)。空なら今と同じ表示である。
- 末尾呼び出し (`TailCall`) は、フレームを積まないので `saved` を持たない。

### インタプリタ

- 環境のスロットは値だけを持つ (`Vec<Option<Value>>`)。読み出しはスロットを書き換えない。`dup` と `decref` はヒープの参照の数だけを増減する。`eml_runtime::Owned` はなくなる。
- 呼び出しでは、`saved` の変数の値だけを `Frame::Return` に退避する。戻ったときは、新しい環境に退避した値と戻り値だけを入れる。
- これで、フレームはちょうど所有している参照だけを持つ。段階3の `drop k` は、フレームを解放するだけで、持っている値を1回ずつ解放できる。

### verifier

呼び出し (`Let` の右辺の `Rhs::Call`) で、引数を使った後に次を確かめる。

- `saved` の変数は範囲の中にある
- RC の対象の変数で所有しているものは、ちょうど `saved` の中の RC の対象の変数で、それぞれ1つずつ所有している
- 呼び出しの後は、`saved` の変数と結果の変数だけが範囲にある

`Jump` では、行き先の join point の本体で使う変数 (RC の対象でないものも含む) が範囲の中にあることを確かめる。呼び出しの後に `Jump` する経路で、join point の本体が使う変数を退避し忘れていないことを見つけるためである。

範囲の取り消しの記録 (R3a の最後の修正) は、範囲から外した変数も記録し、枝と範囲を確かめ終えたら元に戻す。

## 3. 共有されたオブジェクトの複製

- `Payload`、`Frame`、`Closure` から `#[derive(Clone)]` を外す。`ObjRef` を `dup` せずに複製できないようにする。
- `Heap::take_or_copy(obj) -> Result<Payload, HeapError>` を足す。一意なら `take` と同じく解放して中身を返す。共有されていれば中身を写し、写した中身の子をそれぞれ `dup` し、元のオブジェクトを1回解放する。子の列挙は `decref` と同じ関数 (`children`) を使い、写すときと解放するときで数える参照を一致させる。
- インタプリタの `take_closure` は、この手続きを使う。段階3の multi-shot の `resume` も、同じ手続きでフレームを写す。

## 4. 型を付けた実行時エラー

`eml_interp` に次の型を置く。

```rust
pub enum RuntimeError {
    /// 実行中の関数で止まった。
    Fault { fault: Fault, function: String },
    /// `debug_heap` で、終了時に解放されていないオブジェクトがあった。記述子の名前ごとの数。
    Leak(Vec<(String, usize)>),
}

pub enum Fault {
    DivisionByZero,
    IntegerOverflow,
    Heap(HeapError),
    Output(String),
    /// 型検査と Core IR の変換が正しければ起きない誤り。
    Internal(&'static str),
}
```

- `Display` は今と同じ文字列を出す。`Fault` は `division by zero`、`integer overflow`、`HeapError` の表示、`cannot write the output: {error}`、`internal error: {what}` で、`RuntimeError::Fault` はその後に `` in `{function}` `` を付ける。`Leak` は `memory leak: objects were not freed: 1 String` (名前の順に `, ` でつなぐ) である。
- `step` は `Result<Step, Fault>` を返す (`enum Step { Continue, Finished }`)。
- `eml_cli` の `RunResult` をなくし、`execute` は `Result<(), RuntimeError>` を返す。`eml_cli` は `RuntimeError` を再公開する。`main` は今と同じく `runtime error: {error}` を表示する。

## 5. テスト、文書、成功の条件

### 変わるテスト

| テスト | 種類 | 変更 |
|---|---|---|
| `crates/eml_runtime/src/heap.rs` の `registered_descriptors_are_counted_by_name` | 1 | 削除する。`register` をなくすため。段階4で `data` の記述子を足すときに、登録のテストを書き直す |
| `crates/eml_runtime/src/heap.rs` の `a_slot_with_two_references_releases_both` | 1 | 削除する。スロットが複数の参照を持つことがなくなるため。フレームが退避した値を1回ずつ解放することは `decref_releases_children` が確かめる |
| `crates/eml_runtime/src/heap.rs` のほかの単体テスト | 3 | `alloc` の記述子の引数を除き、フレームを `Frame::Return` / `Frame::Apply` で、退避した値を `saved` で組み立てる。期待値は変えない |
| `crates/eml_core_ir/tests/lower.rs` のうち、`saved` が空でない呼び出しを含むスナップショット | 2 | 呼び出しの後ろに `[...]` が付く。どれが変わるかは計画で一覧にする |
| `crates/eml_interp/tests/closures.rs` と `run.rs` の手書きの Core IR | 3 | `Rhs::Call { call, saved }` で組み立て、呼び出しの後で使う変数を `saved` に書く。期待値は変えない |
| `crates/eml_interp/tests/run.rs` の `debug_heap_reports_leaks` | 3 | 期待値を `RuntimeError::Leak(vec![("String".to_string(), 1)])` で書く。意味は変えない |
| `crates/eml_cli/tests/api.rs` の `execute_runs_a_compiled_program` | 3 | 期待値を `Ok(())` で書く。意味は変えない |

UI テスト、HIR と型のスナップショットは変わらない。上の表にないテストの期待値が変わった場合は、変えずに止まり、差分と理由をユーザーに示して承認を得る。

### 足すテスト

- `eml_runtime`: `take_or_copy` が、一意なオブジェクトを取り出し、共有されたオブジェクトを子の参照を数え直して写すこと (写した後で両方を解放するとリークがない)
- `eml_interp`: `RuntimeError` の表示が、今の文字列と同じであること (`Fault` の各種類と `Leak`)
- `eml_core_ir`: 呼び出しの後で使う変数が `saved` に入ること (RC の対象でない変数と、`Jump` の先の join point の本体で使う変数を含む) のスナップショット
- `eml_core_ir`: verifier が、`saved` の過不足 (所有しているのに退避しない、退避したのに所有していない) と、呼び出しの後で退避しなかった変数の使用と、`Jump` の先で使う変数が範囲にないことを拒む

### 文書

| 文書 | 変更 |
|---|---|
| `docs/spec/core-ir.md` | 「インタプリタ (CEK 機械)」の「フレームや環境を解放するときは、まだ残っている値だけを decref すればよい」を、呼び出しのフレームが呼び出しの後で使う変数だけを退避する形に直す。Core IR の呼び出しが退避する変数を持つことを書く |
| `docs/spec/runtime.md` | 「ランタイムの API」に、共有されたオブジェクトの複製 (`take_or_copy`) を足す。記述子をペイロードの種類から決めることを書く |
| `docs/implementation/architecture.md` | 「`eml_core_ir`、`eml_runtime`、`eml_interp` の内部」の `ApplyFrame` の記述と `Owned::refs` の記述を、フレームの種類の enum と退避する変数の形に直す。退避のパスと verifier の検査を足す。`RuntimeError` の型を書く |
| `docs/implementation/status.md` | 「リファクタリング」の表の R3b を完了にする。「テストを変えないために曲げた箇所」の表の1の「今の負担」を「R3b でフレームの種類の enum にした」にする。R3b の一覧を済んだことの1段落にする。「次の作業の注意点」の段階3の `Clone` と `drop k` の注意を、複製の手続きと退避の形に合わせて直す。「完了した作業」に R3b の行を足す |
| `docs/implementation/testing.md` | 「リファクタリング R3b」の見出しを作り、種類1と種類2の変更を記録する |

### 成功の条件

- `Payload::ApplyFrame`、`IO_HANDLER`、`Owned`、`Heap::register` がなく、`Frame` は `Return`、`Apply`、`Io` の enum である
- `Payload`、`Frame`、`Closure` が `Clone` を導出せず、共有されたオブジェクトの複製は `take_or_copy` が行う
- Core IR の呼び出しが退避する変数を持ち、インタプリタはその変数だけをフレームに退避する。verifier がその一致を確かめる
- `RuntimeError` が `Fault` と `Leak` の enum で、`step` が `Result<Step, Fault>` を返し、`eml_cli` に `RunResult` がない
- UI テストの期待値が変わらない。変わったテストは上の表のものだけである
- `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check` が通る
- 上の「文書」の表の変更が済んでいる
