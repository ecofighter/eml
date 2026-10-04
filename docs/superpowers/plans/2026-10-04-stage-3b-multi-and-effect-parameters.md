# 縦の貫通 段階3b Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `multi` の操作と multi-shot の再開、エフェクトの型引数 (`effect State s`、`<State Int | e>`) を、HIR、型検査、ランタイム、インタプリタの全体に通す。

**Architecture:** 上流から順に積む。HIR にエフェクトの `Generics` と型引数つきの row (`EffectRef`) を入れ (Task 1)、型検査で row のラベルに型引数を持たせて単一化する (Task 2)。`multi` は HIR と型検査を一度に通し、`k` を `Unr` にして `return` の節の捕獲に `Unr` の制約を付ける (Task 3)。ランタイムは、共有された継続を `take_or_copy` で区間ごと写し、フレームをつねに一意に保つ (Task 4)。Core IR は変えない。最後に文書を直す (Task 5)。

**Tech Stack:** Rust (edition 2024)、rowan 0.16、la-arena 0.3、insta 1.49。新しい外部 crate は足さない。

**Spec:** `docs/superpowers/specs/2026-10-04-stage-3b-multi-and-effect-parameters-design.md` (段階3b の設計)。規範は `docs/spec/` の `effects.md`、`declarations.md` (`effect`)、`types.md`、`linearity.md`、`core-ir.md`、`runtime.md`、`diagnostics.md`。

## Global Constraints

- 作業は `main` から切ったブランチ `stage-3b` で行う。タスクごとにコミットする。コミットメッセージは英語で、末尾に次の2行を付ける

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01WjhkcJTLXeazUACMXUrKfH
  ```

- 外部 crate は増やさない
- コードのコメントは日本語で、何をするかではなく理由を書く。規則を指すときは `docs/` のパスを書く。日本語を書く前に `yomiyasu:yomiyasu` スキルを読み、その規則に従う (CLAUDE.md)。`.em` のテストの先頭のコメントは、既存のテストと同じく英語で書く
- 各段階は `fn stage(&In) -> (Out, Vec<Diagnostic>)` の純粋関数のままにする
- 診断の help / note は eml 自身の規則の説明に限る。他の言語の書き方を前提にしたヒントは入れない
- インタプリタとランタイムの値とフレームに `Rc` と `RefCell` を使わない。`unsafe` を書かない
- 既存のテストで変えてよいのは、spec の「変わるテスト」の表にあるものだけである。種類1 (`later_stage_effects.em`、`eml_hir/tests/effects.rs` の2件) はこのプランのステップに書いた形に変え、種類3 は期待値を変えずに追随する。表にないテストの期待値が変わったら、変えずに止まり、差分と理由をユーザーに示して承認を得る。設計を曲げてテストを守ることはしない
- 各タスクの最後に `cargo test`、`cargo clippy --all-targets`、`cargo fmt` を通す。clippy の警告を残さない
- インラインスナップショットと UI テストの期待値は、このプランのコードが出す形を書いてある。食い違ったら、まず実装がプランのコードと一致しているかを確かめる。プランの期待値の誤り (位置の数え違い、局所変数の番号など) だと判断した場合は、理由をコミットメッセージに書いてから直す。新しい UI テストのスナップショットは、`cargo insta review` で、ステップに書いた stdout と診断の番号と文言に一致することを確かめてから承認する
- 診断の位置の表記は、1 始まりの行と、文字数で数えた列 (`2:7`) である
- 長い連鎖 (継続の区間) をたどる処理はループで書く。Rust の再帰を使わない
- 途中のタスクでは、ワークスペース全体をビルドできる状態に保つ。Task 1 で `eml_types` に入れる仮の変換 (row の型引数を捨てる) は Task 2 で置き換える。仮の変換には「後で実装する」という趣旨のコメントを書かない

## Review Focus

- `multi` の呼び出しをまたいで `once` の `k` がフレームに退避されたまま、`multi` の継続が2回再開される。段階5の持ち越し規則までは型検査を通るので、ランタイムは `k` の区間も写して、リークも解放済みアクセスもなく動かなければならない → Task 4 の `run/multi_over_once.em`
- 関数型の値の row が、エフェクトの型引数だけ違う (`<Reader Int>` を `<Reader String>` の場所に渡す)。呼び出しの row ではなく、普通の型の不一致 (E2001) として報告されなければならない → Task 2 の `eml_types/tests/effects.rs` の `a_function_type_with_other_effect_arguments_is_a_mismatch`
- `multi` の継続を `drop` する、または使わない。文字列を退避したフレームの区間がリークなく解放されなければならない → Task 4 の `run/multi_resume.em` (`dropped`、`unused`)
- 写す区間に、別のエフェクトの handler フレームが外側につながったまま入っている。写した区間でもその handler が操作を処理し、節のクロージャがリークしない → Task 4 の `run/multi_choice.em`
- 同じエフェクトの handler を違う型引数で入れ子にする。内側が処理し、内側の節から起こした操作は外側の型引数で検査される → Task 2 の `run/effect_parameters.em` (`nested`)

---

### Task 1: HIR にエフェクトの型引数と型引数つきの row を入れる

エフェクトの宣言の型引数を `EffectDef::generics` に、row のエフェクトを `EffectRef` (エフェクトと型引数) にする。型引数の個数の誤り (E1015)、型引数の名前の重複 (E1003)、エフェクトの型引数を結果にした `never` の操作 (E1008) を報告する。`multi` はまだ E0004 のままである (Task 3)。

**Files:**
- Modify: `crates/eml_hir/src/hir.rs`
- Modify: `crates/eml_hir/src/lib.rs` (codes)
- Modify: `crates/eml_hir/src/lower/effect.rs`
- Modify: `crates/eml_hir/src/lower/scope.rs`
- Modify: `crates/eml_hir/src/lower/types.rs`
- Modify: `crates/eml_hir/src/pretty.rs`
- Modify: `crates/eml_types/src/scheme.rs` (仮の変換)
- Modify: `crates/eml_types/src/ty.rs`、`crates/eml_types/src/table/tests.rs` (種類3: `EffectDef` の組み立て)
- Modify: `tests/ui/check-fail/later_stage_effects.em` とスナップショット (種類1)
- Test: `crates/eml_hir/tests/effects.rs`

**Interfaces:**
- Produces:
  - `EffectDef { name: String, generics: Generics, operations: Vec<OperationId> }`
  - `Operation::effect_params: usize` (`signature.generics.type_vars` の先頭の何個がエフェクトの型引数か)
  - `pub struct EffectRef { pub effect: EffectId, pub args: Vec<TypeRefId> }`
  - `RowRef::Closed { effects: Vec<EffectRef>, range }`、`RowRef::Open { effects: Vec<EffectRef>, tail, range }`
  - `eml_hir::codes::TYPE_ARGUMENT_COUNT` (E1015)
  - `ItemScope::define_effect(name, id, params: usize)`、`ItemScope::effect_params(id) -> usize`

- [ ] **Step 1: ブランチを切る**

```bash
git switch -c stage-3b
```

- [ ] **Step 2: 失敗するテストを書く**

`crates/eml_hir/tests/effects.rs` の `effect_type_parameters_and_arguments_come_in_stage_3b` (E0004 を期待するテスト) を削除し、同じ位置に次の3つのテストを書く (種類1の置き換え)。`use` に変更は要らない。

```rust
#[test]
fn effects_take_type_parameters_and_rows_take_type_arguments() {
    let text = "effect State s where\n  get : Unit -> s\n  put : s -> Unit\n  never fail : Unit -> a\n\nf : Unit -> <State Int, State (Int -> Int)> Int\nf () = 1";
    insta::assert_snapshot!(lower_text(text), @r"
    effect State s
      get : Unit -> s
      put : s -> Unit
      never fail : Unit -> a
    f : Unit -> <State Int, State (Int -> Int)> Int
    f () = 1
    ");
}

#[test]
fn operations_see_the_type_parameters_of_their_effect_first() {
    let lowered = lower("effect State s where\n  get : Unit -> s\n  never fail : Unit -> a");
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    let generics = |name: &str| -> (Vec<String>, usize) {
        let (_, operation) = lowered
            .module
            .operations
            .iter()
            .find(|(_, operation)| operation.name == name)
            .unwrap();
        let names = operation
            .signature
            .generics
            .type_vars
            .iter()
            .map(|(_, var)| var.name.clone())
            .collect();
        (names, operation.effect_params)
    };
    assert_eq!(generics("get"), (vec!["s".to_string()], 1));
    assert_eq!(generics("fail"), (vec!["s".to_string(), "a".to_string()], 1));
}

#[test]
fn type_arguments_and_parameters_of_effects_are_checked() {
    let text = "effect State s where\n  get : Unit -> s\n\neffect Pair a a where\n  first : Unit -> a\n\neffect Fail e where\n  never raise : Unit -> e\n\nf : Unit -> <State> Int\nf () = 1\n\ng : Unit -> <State Int Int> Int\ng () = 1\n\nh : Unit -> <State Int> Int\nh () = 1";
    assert_eq!(
        errors(text),
        [
            "E1003 4:15 `a` is defined more than once",
            "E1008 8:25 the result type of a `never` operation must be a type variable that does not appear in its parameters",
            "E1015 10:14 `State` takes 1 type argument, but 0 were given",
            "E1015 13:14 `State` takes 1 type argument, but 2 were given",
        ]
    );
}
```

- [ ] **Step 3: テストが失敗することを確かめる**

Run: `cargo test -p eml_hir --test effects`
Expected: コンパイルエラー (`effect_params` がない)

- [ ] **Step 4: HIR の型を変える**

`crates/eml_hir/src/hir.rs` の `EffectDef`、`Operation`、`RowRef` を次のようにし、`EffectRef` を足す。`Generics` の doc コメントの「型引数を持つエフェクトの宣言には段階3b で持たせる」は「エフェクトの宣言も持つ」に直す。

```rust
#[derive(Debug)]
pub struct EffectDef {
    pub name: String,
    /// 宣言の型引数。エフェクトの引数は型だけで、row 変数は持たない (docs/spec/declarations.md の「`effect`」)。
    pub generics: Generics,
    /// 宣言した順の操作。組み込みの `IO` の操作は組み込みの関数なので、ここには入らない。
    pub operations: Vec<OperationId>,
}
```

```rust
#[derive(Debug)]
pub struct Operation {
    pub name: String,
    pub name_range: TextRange,
    pub effect: EffectId,
    pub multiplicity: OpMultiplicity,
    /// `generics` の先頭の `effect_params` 個は、エフェクトの型引数を写したものである。シグネチャで同じ名前の型変数は
    /// それを指し、ほかの型変数は操作ごとに暗黙に量化する。
    pub signature: Signature,
    pub effect_params: usize,
    /// シグネチャの一番外側の `->` の数 (docs/spec/declarations.md の「`effect`」)。
    pub arity: usize,
}
```

```rust
/// row に書いたエフェクト。型引数の個数は宣言と一致する。違えば E1015 を報告して、row を `RowRef::Error` にする。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectRef {
    pub effect: EffectId,
    pub args: Vec<TypeRefId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowRef {
    /// 省略した row。空の row `<>` である (docs/spec/types.md の「関数型」)。
    Omitted,
    Closed {
        effects: Vec<EffectRef>,
        range: TextRange,
    },
    /// `<e>` と `<IO | e>`。
    Open {
        effects: Vec<EffectRef>,
        tail: RowVarId,
        range: TextRange,
    },
    /// 未定義のエフェクトか、解決できない row 変数の跡。型検査はどのエフェクトも受け入れ、診断を連鎖させない。
    Error,
}
```

`crates/eml_hir/src/lib.rs` の `codes` に足す。

```rust
    pub const TYPE_ARGUMENT_COUNT: ErrorCode = ErrorCode(1015);
```

- [ ] **Step 5: `ItemScope` にエフェクトの型引数の個数を持たせる**

`crates/eml_hir/src/lower/scope.rs`:

```rust
#[derive(Debug, Default)]
pub(super) struct ItemScope {
    /// ユーザーが定義した値 (関数と操作)。
    values: HashMap<String, ValueItem>,
    types: HashMap<String, TypeItem>,
    /// エフェクトの型引数の個数。row のエフェクトの型引数の個数を確かめるのに使う (E1015)。
    effect_params: HashMap<EffectId, usize>,
}
```

```rust
    pub(super) fn define_effect(&mut self, name: &str, id: EffectId, params: usize) {
        self.types.insert(name.to_string(), TypeItem::Effect(id));
        self.effect_params.insert(id, params);
    }

    pub(super) fn effect_params(&self, id: EffectId) -> usize {
        self.effect_params.get(&id).copied().unwrap_or(0)
    }
```

`builtin_items` の `IO` は次のようにする。

```rust
    let io = effects.alloc(EffectDef {
        name: "IO".to_string(),
        generics: Generics::default(),
        operations: Vec::new(),
    });
    scope.define_effect("IO", io, 0);
```

`use crate::hir::{...}` に `Generics` を足す。

- [ ] **Step 6: 宣言の型引数と操作の `Generics` を変換する**

`crates/eml_hir/src/lower/effect.rs` の `lower_effects` の最初のループで、E0004 を出していた `for param in item.params()` を次に置き換え、`EffectDef` の組み立てと `define_effect` を直す。

```rust
        let mut generics = Generics::default();
        for param in item.params() {
            let text = param.text();
            let range = param.text_range();
            if let Some((_, first)) = generics.type_vars.iter().find(|(_, var)| var.name == text) {
                diagnostics.push(duplicate(file, text, first.range, range));
                continue;
            }
            generics.type_vars.alloc(TypeVarDecl {
                name: text.to_string(),
                range,
            });
        }
        let params = generics.type_vars.len();
        let range = name.text_range();
        let id = effects.alloc(EffectDef {
            name: name.text().to_string(),
            generics,
            operations: Vec::new(),
        });
        match declared.get(name.text()) {
            Some(&first) => diagnostics.push(duplicate(file, name.text(), first, range)),
            None => {
                declared.insert(name.text().to_string(), range);
                scope.define_effect(name.text(), id, params);
            }
        }
```

2つ目のループの呼び出しを `lower_operation(file, &decl, effect, &effects[effect].generics, scope, diagnostics)` にする。`lower_operation` は次のようにする。

```rust
fn lower_operation(
    file: FileId,
    decl: &ast::OpDecl,
    effect: EffectId,
    effect_generics: &Generics,
    scope: &ItemScope,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Operation> {
    // 名前がなければパーサが報告済み
    let name = decl.name()?;
    let multiplicity = match decl.multiplicity() {
        Some(token) if token.kind() == SyntaxKind::NEVER_KW => OpMultiplicity::Never,
        Some(token) if token.kind() == SyntaxKind::MULTI_KW => {
            diagnostics.push(Diagnostic::not_yet_supported(
                file,
                token.text_range(),
                "`multi` operations are not supported yet",
            ));
            OpMultiplicity::Once
        }
        _ => OpMultiplicity::Once,
    };
    let range = decl.ty().map_or(decl.range(), |ty| ty.range());
    let mut types = Arena::new();
    // エフェクトの型引数を先頭に写す。シグネチャで同じ名前の型変数は、それを指す (docs/spec/declarations.md の「`effect`」)
    let mut generics = Generics::default();
    for (_, param) in effect_generics.type_vars.iter() {
        generics.type_vars.alloc(param.clone());
    }
    let effect_params = generics.type_vars.len();
    let ty = TypeLowering {
        file,
        types: &mut types,
        generics: &mut generics,
        items: scope,
        define: true,
        diagnostics: &mut *diagnostics,
    }
    .lower(decl.ty(), range);
    let signature = Signature {
        ty,
        range,
        types,
        generics,
    };
    let arity = check_signature(
        file,
        name.text(),
        &signature,
        multiplicity,
        effect_params,
        diagnostics,
    );
    Some(Operation {
        name: name.text().to_string(),
        name_range: name.text_range(),
        effect,
        multiplicity,
        signature,
        effect_params,
        arity,
    })
}
```

`check_signature` に `effect_params: usize` を足し (`multiplicity` の後)、`never` の検査を次にする。エフェクトの型引数を結果にしたときは、既存の文言のまま、ラベルだけを変える。既存の E1008 のテストの出力は変わらない。

```rust
    if multiplicity == OpMultiplicity::Never {
        let effect_param = match types[id].kind {
            TypeRefKind::Var(var) => (u32::from(var.into_raw()) as usize) < effect_params,
            _ => false,
        };
        let free = match types[id].kind {
            // エフェクトの型引数は handle ごとに決まるので、呼び出した側が自由な型として使えない
            TypeRefKind::Var(_) if effect_param => false,
            TypeRefKind::Var(var) => !params.iter().any(|&param| mentions(types, param, var)),
            TypeRefKind::Error => true,
            TypeRefKind::Con(_) | TypeRefKind::Fn { .. } => false,
        };
        if !free {
            let label = if effect_param {
                "this is a type parameter of the effect"
            } else {
                "this result type"
            };
            diagnostics.push(
                Diagnostic::error(
                    codes::NEVER_RESULT_NOT_FREE,
                    "the result type of a `never` operation must be a type variable that does not appear in its parameters",
                    Label::new(file, types[id].range, label),
                )
                .with_note(
                    "a `never` operation does not return, so its caller may use the result as any type",
                ),
            );
        }
    }
```

`use crate::hir::{...}` に `TypeVarDecl` を足す。

- [ ] **Step 7: row の型引数を変換する**

`crates/eml_hir/src/lower/types.rs` の `row` の `for effect in row.effects()` のループを次に置き換える。`use crate::hir::{...}` に `EffectRef` を足す。

```rust
        let mut effects = Vec::new();
        for effect in row.effects() {
            let Some(name) = effect.name() else {
                continue;
            };
            match self.items.type_item(name.text()) {
                Some(TypeItem::Effect(id)) => {
                    let args: Vec<TypeRefId> = effect
                        .args()
                        .map(|arg| {
                            let range = arg.range();
                            self.lower(Some(arg), range)
                        })
                        .collect();
                    let expected = self.items.effect_params(id);
                    if args.len() != expected {
                        let given = match args.len() {
                            1 => "1 was given".to_string(),
                            n => format!("{n} were given"),
                        };
                        self.diagnostics.push(Diagnostic::error(
                            codes::TYPE_ARGUMENT_COUNT,
                            format!(
                                "`{}` takes {}, but {given}",
                                name.text(),
                                type_arguments(expected)
                            ),
                            Label::new(
                                self.file,
                                effect.range(),
                                format!("expected {}", type_arguments(expected)),
                            ),
                        ));
                        valid = false;
                        continue;
                    }
                    effects.push(EffectRef { effect: id, args });
                }
                Some(TypeItem::Type(_)) | None => {
                    self.diagnostics.push(Diagnostic::error(
                        codes::UNDEFINED_TYPE,
                        format!("cannot find effect `{}`", name.text()),
                        Label::new(self.file, effect.range(), "not found in this scope"),
                    ));
                    valid = false;
                }
            }
        }
```

ファイルの末尾に足す。

```rust
fn type_arguments(n: usize) -> String {
    if n == 1 {
        "1 type argument".to_string()
    } else {
        format!("{n} type arguments")
    }
}
```

- [ ] **Step 8: HIR の表示を直す**

`crates/eml_hir/src/pretty.rs` の `pretty` で、エフェクトの見出しに型引数を出す。

```rust
        let params: Vec<&str> = effect
            .generics
            .type_vars
            .iter()
            .map(|(_, var)| var.name.as_str())
            .collect();
        if params.is_empty() {
            writeln!(out, "effect {}", effect.name).unwrap();
        } else {
            writeln!(out, "effect {} {}", effect.name, params.join(" ")).unwrap();
        }
```

`Printer::ty` の `effect_names` を、型引数も出す形にする (関数型の引数は括弧で囲む)。

```rust
                let effect_names = |effects: &[EffectRef]| -> Vec<String> {
                    effects
                        .iter()
                        .map(|effect| {
                            let mut text = self.module.effects[effect.effect].name.clone();
                            for &arg in &effect.args {
                                let arg_text = self.ty(types, arg);
                                if matches!(types[arg].kind, TypeRefKind::Fn { .. }) {
                                    write!(text, " ({arg_text})").unwrap();
                                } else {
                                    write!(text, " {arg_text}").unwrap();
                                }
                            }
                            text
                        })
                        .collect()
                };
```

- [ ] **Step 9: `eml_types` をビルドできるようにする (仮の変換と種類3)**

`crates/eml_types/src/scheme.rs` の `lower` の row の変換で、エフェクトの ID だけを取り出す。Task 2 で型引数を持つラベルに置き換える。

```rust
                RowRef::Closed { effects, .. } => {
                    Row::closed(effects.iter().map(|effect| effect.effect).collect())
                }
                RowRef::Open { effects, tail, .. } => Row {
                    labels: effects.iter().map(|effect| effect.effect).collect(),
                    tail: Tail::Var(rigids.rows[*tail]),
                },
```

`crates/eml_types/src/ty.rs` のテスト2か所と `crates/eml_types/src/table/tests.rs` の `new_table` の `EffectDef` の組み立てに `generics: Generics::default(),` を足し、`use eml_hir::{...}` に `Generics` を足す (種類3)。

- [ ] **Step 10: `later_stage_effects.em` を `from` だけにする (種類1)**

`tests/ui/check-fail/later_stage_effects.em` の全体を次にする。`multi` は Task 3 で通すので、ここで一緒に外す。

```haskell
-- E0004: handlers with `from` come in a later stage.
counter : Unit -> Int
counter () = 0

with_state : Int -> Int
with_state n =
  handle counter () from n with
    | return x st -> x
```

- [ ] **Step 11: テストを通す**

Run: `cargo test -p eml_hir --test effects`
Expected: PASS

Run: `cargo test`
Expected: `ui::check_fail` が `later_stage_effects.em` のスナップショットの差分で失敗する。`cargo insta review` で、E0004 が `handlers with \`from\` are not supported yet` の1件だけ (位置 `7:21`) になったことを確かめて承認し、もう一度 `cargo test` を通す。ほかのテストは変わらない。

- [ ] **Step 12: clippy と fmt を通してコミットする**

```bash
cargo clippy --all-targets && cargo fmt
git add -A
git commit -m "Lower effect type parameters and rows with type arguments to HIR

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01WjhkcJTLXeazUACMXUrKfH"
```

---

### Task 2: row のラベルに型引数を持たせて型検査する

型の表の row のラベルを `Label { effect, args }` にし、scoped labels の単一化で同じエフェクトのラベルを順に対にして型引数を単一化する。操作のスキームの row に型引数を付け、handle ごとにエフェクトの型引数を新しい変数にする。呼び出しの row の型引数の不一致は E2001 にする。

**Files:**
- Modify: `crates/eml_types/src/table/mod.rs`、`row.rs`、`unify.rs`、`copy.rs`、`kinds.rs`、`export.rs`、`tests.rs`
- Modify: `crates/eml_types/src/ty.rs`
- Modify: `crates/eml_types/src/scheme.rs`
- Modify: `crates/eml_types/src/check/mod.rs`、`handle.rs`、`report.rs`
- Test: `crates/eml_types/tests/effects.rs`、`crates/eml_types/src/table/tests.rs`、`crates/eml_types/src/ty.rs`
- Create: `tests/ui/run/effect_parameters.em`、`tests/ui/check-fail/effect_arguments.em`

**Interfaces:**
- Consumes: Task 1 の `EffectDef::generics`、`Operation::effect_params`、`EffectRef`
- Produces:
  - `pub(crate) struct Label { pub effect: EffectId, pub args: Vec<Ty> }` (`table/mod.rs`)。`Row::labels: Vec<Label>`、`Row::closed(labels: Vec<Label>)`
  - `UnifyError::EffectArgs { left: Label, right: Label }` (`left` は単一化の左辺の row のラベル)
  - `Table::display_label(&self, label: &Label) -> EffectLabel`
  - `Table::unrestricted(&mut self, ty: Ty, keep: &[KindVar])`
  - `pub struct EffectLabel { pub id, pub name, pub args: Vec<Type> }` と `impl Display for EffectLabel`
  - `Rigids::with_effect_args(table, generics, effect_args: &[Ty]) -> Rigids`、`Rigids::effect_args(&self, operation: &Operation) -> Vec<Ty>`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/tests/effects.rs` の末尾に足す。

```rust
#[test]
fn type_parameters_of_an_effect_are_not_fixed_to_unr() {
    let text = "effect State s where\n  get : Unit -> s\n  put : s -> Unit\n\neffect Store s where\n  store : a -> s -> Unit\n\ncounter : Unit -> <State Int> Int\ncounter () =\n  put (get () + 1)\n  get ()";
    insta::assert_snapshot!(check_text(text), @r"
    get : Unit -> <State s> s
    put : s -> <State s> Unit
    store : a -> s -> <Store s> Unit
      kinds: a <= Unr
    counter : Unit -> <State Int> Int
    ");
}

#[test]
fn type_arguments_of_a_performed_effect_must_match_the_row() {
    let text = "effect State s where\n  get : Unit -> s\n  put : s -> Unit\n\nwrong : Unit -> <State Int> Unit\nwrong () = put \"text\"";
    insta::assert_snapshot!(check_text(text), @r"
    get : Unit -> <State s> s
    put : s -> <State s> Unit
    wrong : Unit -> <State Int> Unit
    ---
    E2001 6:12 `put` performs `State String`, but the row allows `State Int`
      6:12 this call performs `State String`
      note: the type arguments of an effect must match those in the row
    ");
}

#[test]
fn a_handler_takes_the_type_arguments_of_its_effect_from_the_body() {
    let text = "effect Reader r where\n  ask : Unit -> r\n\ngreeting : Unit -> <Reader String> String\ngreeting () = ask ()\n\nrun : Unit -> String\nrun () =\n  handle greeting () with\n    | ask () k -> resume k \"x\"\n\nwrong : Unit -> String\nwrong () =\n  handle greeting () with\n    | ask () k -> resume k 1";
    insta::assert_snapshot!(check_text(text), @r"
    ask : Unit -> <Reader r> r
    greeting : Unit -> <Reader String> String
    run : Unit -> String
      k#0 : Cont String String <>
    wrong : Unit -> String
      k#0 : Cont String String <>
    ---
    E2001 15:28 mismatched types
      15:28 expected `String`, found `Int`
      note: `resume` passes this value as the result of the operation
    ");
}

#[test]
fn a_function_type_with_other_effect_arguments_is_a_mismatch() {
    // `f` を `drop` するのは、使わない引数の `Unr` の制約を `kinds:` の行に出さないため
    let text = "effect Reader r where\n  ask : Unit -> r\n\nrun : (Unit -> <Reader String> Int) -> Int\nrun f =\n  drop f\n  0\n\nnumber : Unit -> <Reader Int> Int\nnumber () = ask ()\n\napply_it : Unit -> Int\napply_it () = run number";
    insta::assert_snapshot!(check_text(text), @r"
    ask : Unit -> <Reader r> r
    run : (Unit -> <Reader String> Int) -> Int
      f#0 : Unit -> <Reader String> Int
    number : Unit -> <Reader Int> Int
    apply_it : Unit -> Int
    ---
    E2001 13:19 mismatched types
      13:19 expected `Unit -> <Reader String> Int`, found `Unit -> <Reader Int | _> Int`
      13:15 argument 1 of `run`
    ");
}
```

`crates/eml_types/src/table/tests.rs` の末尾に足す。

```rust
#[test]
fn labels_of_one_effect_pair_up_in_order() {
    let mut table = new_table();
    let io = table.lang.io;
    let label = |args: Vec<Ty>| Label { effect: io, args };
    let a = Row::closed(vec![label(vec![table.int]), label(vec![table.string])]);
    let x = table.fresh_var();
    let y = table.fresh_var();
    let b = Row::closed(vec![label(vec![x]), label(vec![y])]);
    assert_eq!(table.unify_row(&a, &b), Ok(()));
    assert_eq!(table.display(x).to_string(), "Int");
    assert_eq!(table.display(y).to_string(), "String");
}

#[test]
fn labels_with_different_type_arguments_do_not_unify() {
    let mut table = new_table();
    let io = table.lang.io;
    let left = Label {
        effect: io,
        args: vec![table.string],
    };
    let right = Label {
        effect: io,
        args: vec![table.int],
    };
    assert_eq!(
        table.unify_row(&Row::closed(vec![left.clone()]), &Row::closed(vec![right.clone()])),
        Err(UnifyError::EffectArgs { left, right })
    );
}
```

`crates/eml_types/src/ty.rs` の `mod tests` に足す。

```rust
    #[test]
    fn effect_labels_are_displayed_with_their_type_arguments() {
        let mut types = Arena::new();
        let mut effects = Arena::new();
        let int = Type::Con {
            id: types.alloc(TypeDef {
                name: "Int".to_string(),
            }),
            name: "Int".to_string(),
        };
        let id = effects.alloc(EffectDef {
            name: "State".to_string(),
            generics: Generics::default(),
            operations: Vec::new(),
        });
        let function = Type::Fn {
            param: Box::new(int.clone()),
            linearity: Linearity::Unr,
            effects: vec![],
            tail: None,
            ret: Box::new(int.clone()),
        };
        let ty = Type::Fn {
            param: Box::new(Type::unit()),
            linearity: Linearity::Unr,
            effects: vec![
                EffectLabel {
                    id,
                    name: "State".to_string(),
                    args: vec![int.clone()],
                },
                EffectLabel {
                    id,
                    name: "State".to_string(),
                    args: vec![function],
                },
            ],
            tail: Some(RowTail::Rigid("e".to_string())),
            ret: Box::new(int),
        };
        assert_eq!(
            ty.to_string(),
            "Unit -> <State Int, State (Int -> Int) | e> Int"
        );
    }
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_types`
Expected: コンパイルエラー (`Label`、`EffectLabel::args`、`UnifyError::EffectArgs` がない)

- [ ] **Step 3: `Label` と `UnifyError::EffectArgs` を足す**

`crates/eml_types/src/table/mod.rs`:

```rust
/// row のラベル。エフェクトとその型引数である (docs/spec/types.md の「関数型」)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Label {
    pub effect: EffectId,
    pub args: Vec<Ty>,
}

#[cfg(test)]
impl Label {
    /// 型引数のないラベル。表の単体テストで row を組み立てるために使う。
    pub fn plain(effect: EffectId) -> Label {
        Label {
            effect,
            args: Vec::new(),
        }
    }
}

/// エフェクトの row。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Row {
    pub labels: Vec<Label>,
    pub tail: Tail,
}
```

`Row::closed` の引数を `labels: Vec<Label>` にする。`UnifyError` に足す。

```rust
    /// 同じエフェクトのラベルの型引数が一致しない。`left` は単一化の左辺の row のラベルである。
    EffectArgs { left: Label, right: Label },
```

- [ ] **Step 4: row の単一化で型引数を単一化する**

`crates/eml_types/src/table/row.rs`:

`resolve_row` の `labels.extend(bound.labels.iter().copied())` を `labels.extend(bound.labels.iter().cloned())` にする。

`unify_row` の先頭を次にし、以降の `UnifyError::MissingEffects(only_a)` などは、ラベルの並びから ID を取り出す `missing` で作る。

```rust
    pub fn unify_row(&mut self, a: &Row, b: &Row) -> Result<(), UnifyError> {
        let a = self.resolve_row(a);
        let b = self.resolve_row(b);
        let mut only_b = b.labels.clone();
        let mut only_a = Vec::new();
        let mut pairs = Vec::new();
        // 同じエフェクトのラベルが複数あれば、それぞれの row の中の順で対にする (scoped labels)
        for label in &a.labels {
            match only_b.iter().position(|other| other.effect == label.effect) {
                Some(index) => pairs.push((label.clone(), only_b.remove(index))),
                None => only_a.push(label.clone()),
            }
        }
        for (left, right) in pairs {
            let args: Vec<(Ty, Ty)> = left.args.iter().copied().zip(right.args.iter().copied()).collect();
            for (x, y) in args {
                if self.unify(x, y).is_err() {
                    return Err(UnifyError::EffectArgs { left, right });
                }
            }
        }
        match (a.tail, b.tail) {
            // (今の分岐をそのまま残し、`MissingEffects(...)` を `missing(&...)` に替える)
        }
    }
```

`(Tail::Closed, Tail::Closed)` と `(Tail::Var(x), Tail::Var(y)) if x == y` の分岐は次の形にする。

```rust
            (Tail::Closed, Tail::Closed) => {
                let mut rest = only_a;
                rest.extend(only_b);
                if rest.is_empty() {
                    Ok(())
                } else {
                    Err(missing(&rest))
                }
            }
```

ファイルの末尾に足す。

```rust
fn missing(labels: &[Label]) -> UnifyError {
    UnifyError::MissingEffects(labels.iter().map(|label| label.effect).collect())
}
```

`bind_row` の多重度の下限を `self.effect_multiplicity(label.effect)` にする。

- [ ] **Step 5: occurs、写し、Kind 変数、書き出しでラベルの型引数をたどる**

`crates/eml_types/src/table/unify.rs` の `occurs`:

```rust
            TyShape::Fn {
                param, row, ret, ..
            }
            | TyShape::Cont {
                arg: param,
                row,
                ret,
                ..
            } => {
                self.occurs(var, *param)
                    || self.occurs(var, *ret)
                    || self
                        .resolve_row(row)
                        .labels
                        .iter()
                        .any(|label| label.args.iter().any(|&arg| self.occurs(var, arg)))
            }
```

`crates/eml_types/src/table/copy.rs` に `copy_row` を足し、`Fn` と `Cont` の分岐の row の写しをそれに替える。

```rust
    /// row の末尾の row 変数と、ラベルの型引数を `subst` に従って置き換える。
    fn copy_row(&mut self, row: &Row, subst: &Subst) -> Row {
        let row = self.resolve_row(row);
        let mut labels = Vec::new();
        for label in row.labels {
            let mut args = Vec::new();
            for arg in label.args {
                args.push(self.copy_type(arg, subst));
            }
            labels.push(Label {
                effect: label.effect,
                args,
            });
        }
        let tail = match row.tail {
            Tail::Var(tail) => Tail::Var(subst.rows.get(&tail).copied().unwrap_or(tail)),
            other => other,
        };
        Row { labels, tail }
    }
```

`crates/eml_types/src/table/kinds.rs` の `kind_vars` の `Fn` と `Cont` の分岐では、row を一度だけ解決し、末尾の多重度の後に、引数、ラベルの型引数、戻り値の順に見るよう積む (作業リストは後ろから取り出す)。

```rust
                    let row = self.resolve_row(row);
                    if let Tail::Var(tail) = row.tail
                        && self.is_rigid_row(tail)
                    {
                        push_unique(&mut mult, self.row_multiplicity_var(tail));
                    }
                    work.push(*ret);
                    for label in row.labels.iter().rev() {
                        work.extend(label.args.iter().rev().copied());
                    }
                    work.push(*param);
```

(`Cont` では `param` が `arg` である。) `unrestricted` は残す Kind 変数を受け取る。

```rust
    /// `ty` に現れる線形性の Kind 変数 (rigid 変数の `μ` と矢印の `m`) を、`keep` を除いて `Unr` 以下にする。操作の
    /// 引数の型に使う。操作は本体を持たないので、節がその引数をどう使うかを操作の型に推論できない。そこで `Unr` に固定し、
    /// 節では引数を何回使ってもよいことにする。エフェクトの型引数 (`keep`) は handle ごとに具体的な型で節を検査するので
    /// 固定しない (docs/spec/effects.md の「handler の意味」)。
    pub fn unrestricted(&mut self, ty: Ty, keep: &[KindVar]) {
        let (lin, _) = self.kind_vars(ty);
        for var in lin.into_iter().filter(|var| !keep.contains(var)) {
            self.linearity
                .require(Bound::Var(var), Bound::Const(Linearity::Unr));
        }
    }
```

`crates/eml_types/src/table/export.rs`:
- `kind_names` の `Fn` と `Cont` の分岐で、`kind_vars` と同じく、`ret` の後、`param` の前にラベルの型引数を積む。
- `export_row` に `solved: bool` を足し、`to_type` から `self.export_row(&row, solved)` と呼ぶ。ラベルは `label_type` で作る。

```rust
    /// 診断の文言のためのラベルの形。
    pub fn display_label(&self, label: &Label) -> EffectLabel {
        self.label_type(label, false)
    }

    fn label_type(&self, label: &Label, solved: bool) -> EffectLabel {
        EffectLabel {
            id: label.effect,
            name: self.effect_names[label.effect].clone(),
            args: label
                .args
                .iter()
                .map(|&arg| self.to_type(arg, solved))
                .collect(),
        }
    }

    fn export_row(&self, row: &Row, solved: bool) -> (Vec<EffectLabel>, Option<RowTail>) {
        let row = self.resolve_row(row);
        let effects = row
            .labels
            .iter()
            .map(|label| self.label_type(label, solved))
            .collect();
        // (末尾の扱いは今のまま)
    }
```

- [ ] **Step 6: 外に出す型のラベルに型引数を持たせる**

`crates/eml_types/src/ty.rs`:

```rust
/// 外に出す型の row のラベル。名前を持つのは、`Module` を渡さずに表示するため。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectLabel {
    pub id: EffectId,
    pub name: String,
    pub args: Vec<Type>,
}

impl fmt::Display for EffectLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)?;
        for arg in &self.args {
            write!(f, " {}", atomic(arg))?;
        }
        Ok(())
    }
}
```

`row_text` の名前の並びを `effects.iter().map(|e| e.to_string()).collect::<Vec<String>>()` にする。`contains_error` の `Fn` と `Cont` に、ラベルの型引数の誤りを足す (`effects.iter().any(|e| e.args.iter().any(Type::contains_error))`)。既存の2つのテストの `EffectLabel` の組み立てに `args: vec![]` を足す (種類3)。

- [ ] **Step 7: スキームの row に型引数を付ける**

`crates/eml_types/src/scheme.rs`:

```rust
impl Rigids {
    pub fn new(table: &mut Table, generics: &Generics) -> Rigids {
        Rigids::with_effect_args(table, generics, &[])
    }

    /// 先頭の型変数を `effect_args` の型にし、残りを新しい rigid 変数にする。handler の操作の節で、エフェクトの型引数を
    /// handle の型引数に、操作自身の型変数を rigid にするために使う (docs/spec/effects.md の「handler の意味」)。
    pub fn with_effect_args(table: &mut Table, generics: &Generics, effect_args: &[Ty]) -> Rigids {
        let mut rigids = Rigids {
            tys: ArenaMap::default(),
            rows: ArenaMap::default(),
            vars: Vec::new(),
        };
        for (index, (id, var)) in generics.type_vars.iter().enumerate() {
            if let Some(&arg) = effect_args.get(index) {
                rigids.tys.insert(id, arg);
                continue;
            }
            let (ty, rigid) = table.fresh_rigid(&var.name);
            rigids.tys.insert(id, ty);
            rigids.vars.push(rigid);
        }
        for (id, var) in generics.row_vars.iter() {
            rigids.rows.insert(id, table.fresh_rigid_row(&var.name));
        }
        rigids
    }

    /// 操作の `Generics` の先頭に写した、エフェクトの型引数の型。
    pub fn effect_args(&self, operation: &Operation) -> Vec<Ty> {
        operation
            .signature
            .generics
            .type_vars
            .iter()
            .take(operation.effect_params)
            .map(|(id, _)| self.tys[id])
            .collect()
    }
}
```

`lower` の row の変換 (Task 1 の仮の変換) を、型引数を型にしたラベルに置き換える。

```rust
                RowRef::Closed { effects, .. } => {
                    Row::closed(lower_labels(table, types, rigids, effects))
                }
                RowRef::Open { effects, tail, .. } => Row {
                    labels: lower_labels(table, types, rigids, effects),
                    tail: Tail::Var(rigids.rows[*tail]),
                },
```

```rust
fn lower_labels(
    table: &mut Table,
    types: &Arena<TypeRef>,
    rigids: &Rigids,
    effects: &[EffectRef],
) -> Vec<Label> {
    let mut labels = Vec::new();
    for effect in effects {
        let mut args = Vec::new();
        for &arg in &effect.args {
            args.push(lower(table, types, rigids, arg, false));
        }
        labels.push(Label {
            effect: effect.effect,
            args,
        });
    }
    labels
}
```

`lower_operation` と `with_effect` は、エフェクトの型引数を持つラベルを付ける。

```rust
pub(crate) fn lower_operation(table: &mut Table, operation: &Operation, rigids: &Rigids) -> Ty {
    let ty = lower_signature(table, &operation.signature, rigids);
    let label = Label {
        effect: operation.effect,
        args: rigids.effect_args(operation),
    };
    with_effect(table, ty, operation.arity, label)
}

fn with_effect(table: &mut Table, ty: Ty, arity: usize, label: Label) -> Ty {
    // (今の形のまま、`labels: vec![effect]` を `labels: vec![label]` に、再帰の引数を `label` にする)
}
```

`use` を直す (`EffectId` を外し、`EffectRef` と `table::Label` を足す)。

- [ ] **Step 8: 操作のスキームでエフェクトの型引数を固定しない**

`crates/eml_types/src/check/mod.rs` の操作のループ:

```rust
    for (id, operation) in module.operations.iter() {
        let rigids = Rigids::new(&mut table, &operation.signature.generics);
        let ty = lower_operation(&mut table, operation, &rigids);
        table.closure_kinds(ty, operation.arity, &[]);
        // エフェクトの型引数は handle ごとに具体的な型で節を検査するので、`Unr` に固定しない
        let effect_kinds: Vec<KindVar> = rigids
            .effect_args(operation)
            .into_iter()
            .flat_map(|ty| table.kind_bounds(ty))
            .filter_map(|bound| match bound {
                Bound::Var(var) => Some(var),
                Bound::Const(_) => None,
            })
            .collect();
        // 結果の型は縛らない。`never fail : String -> a` をどの型としても使えるようにするため
        let mut spine = ty;
        for _ in 0..operation.arity {
            let TyShape::Fn { param, ret, .. } = table.shape(spine).clone() else {
                break;
            };
            table.unrestricted(param, &effect_kinds);
            spine = ret;
        }
        let mut scheme = Scheme::new(ty, &rigids);
        scheme.generalize(&table);
        operations.insert(id, scheme);
    }
```

`use crate::kind::{Bound, KindVar};` にする。`check_main` の期待する `EffectLabel` に `args: Vec::new()` を足す。`has_error` は row のエフェクトの型引数も見る。

```rust
        TypeRefKind::Fn { param, row, ret } => {
            let args_error = match row {
                RowRef::Closed { effects, .. } | RowRef::Open { effects, .. } => effects
                    .iter()
                    .any(|effect| effect.args.iter().any(|&arg| has_error(types, arg))),
                RowRef::Omitted | RowRef::Error => false,
            };
            matches!(row, RowRef::Error)
                || args_error
                || has_error(types, *param)
                || has_error(types, *ret)
        }
```

- [ ] **Step 9: handle ごとにエフェクトの型引数を新しい変数にする**

`crates/eml_types/src/check/handle.rs` の `handle`:

```rust
        let outer = self.ambient.clone();
        // handle ごとにエフェクトの型引数を新しい変数にする。本体の操作の呼び出しと節が、この変数を通じて型引数を共有する
        let module = self.module;
        let args: Vec<Ty> = module.effects[effect]
            .generics
            .type_vars
            .iter()
            .map(|_| self.table.fresh_var())
            .collect();
        // scoped labels なので、同じエフェクトの handler を入れ子にすると内側が処理する
        let inner = Row {
            labels: std::iter::once(Label {
                effect,
                args: args.clone(),
            })
            .chain(outer.labels.iter().cloned())
            .collect(),
            tail: outer.tail,
        };
```

節の呼び出しを `self.op_clause(clause, result, &outer, &args);` にし、`op_clause` を次にする (シグネチャに `effect_args: &[Ty]` を足す)。

```rust
    /// 節の引数は操作の引数の型で、`k` は「操作の結果を受け、handle 式の値を返し、外側の row のエフェクトを起こす」
    /// 継続である。`once` の操作の `k` は `Lin` である (docs/spec/effects.md の「継続の多重度と持ち越し規則」)。
    fn op_clause(&mut self, clause: &OpClause, result: Ty, outer: &Row, effect_args: &[Ty]) {
        let operation = &self.module.operations[clause.op];
        // エフェクトの型引数は handle の型引数である。操作自身の型変数は節の中では rigid である。handler は、操作が
        // どの型で呼ばれても動かなければならないため
        let rigids = Rigids::with_effect_args(self.table, &operation.signature.generics, effect_args);
        let mut ty = lower_operation(self.table, operation, &rigids);
        // (以降は今のまま)
```

`use crate::table::{...}` に `Label` を足す。

- [ ] **Step 10: 呼び出しの row の型引数の不一致を E2001 にする**

`crates/eml_types/src/check/report.rs` の `include_call_row` の `match` に、`Err(other) => unreachable!(...)` の前に次の分岐を足す。`unreachable!` のコメントは「include_row は呼び出し先側の rigid でない row 変数を通してしか単一化しないので、rigid 変数の束縛 (Mismatch) も Occurs も起きない。型引数の誤りは `EffectArgs` になる」とする。

```rust
            Err(UnifyError::EffectArgs { left, right }) => {
                // 呼び出し先の row が左辺である (`Table::include_row`)
                if report {
                    let found = self.table.display_label(&left);
                    let allowed = self.table.display_label(&right);
                    self.diagnostics.push(
                        Diagnostic::error(
                            codes::TYPE_MISMATCH,
                            format!("{name} performs `{found}`, but the row allows `{allowed}`"),
                            Label::new(self.file(), range, format!("this call performs `{found}`")),
                        )
                        .with_note("the type arguments of an effect must match those in the row"),
                    );
                }
                return false;
            }
```

- [ ] **Step 11: 既存の表のテストを `Label` に追随させる (種類3)**

`crates/eml_types/src/table/tests.rs` で、`Row::closed(vec![io])` を `Row::closed(vec![Label::plain(io)])` に、`labels: vec![io]` を `labels: vec![Label::plain(io)]` に、`assert_eq!(a.labels, vec![io])` を `assert_eq!(a.labels, vec![Label::plain(io)])` にする。`MissingEffects(vec![io])` は ID の並びのままである。期待値は変えない。

- [ ] **Step 12: crate のテストを通す**

Run: `cargo test -p eml_types`
Expected: PASS (新しい6つのテストを含む)

- [ ] **Step 13: UI テストを足す**

`tests/ui/run/effect_parameters.em`:

```haskell
-- Effects with type parameters. A handler that returns a function implements a state, `Reader` is used with
-- `String`, a function polymorphic in the type argument runs under handlers of different types, and with nested
-- handlers of one effect the inner one handles the operations while its clause performs at the outer one.
effect State s where
  get : Unit -> s
  put : s -> Unit

effect Reader r where
  ask : Unit -> r

counter : Unit -> <State Int> Int
counter () =
  put (get () + 1)
  put (get () * 10)
  get ()

run_counter : Int -> Int
run_counter start =
  let run =
    handle counter () with
      | get () k -> fn s -> (resume k s) s
      | put n k -> fn _ -> (resume k ()) n
      | return x -> fn _ -> x
  run start

greeting : Unit -> <Reader String> String
greeting () = "hello " ++ ask ()

with_name : String -> String
with_name name =
  handle greeting () with
    | ask () k -> resume k name

both_ways : (r -> r -> String) -> <Reader r> String
both_ways f = f (ask ()) (ask ())

nested : Unit -> String
nested () =
  handle (handle greeting () with | ask () k -> resume k (show_int (ask ()))) with
    | ask () k -> resume k 7

main : Unit -> <IO> Unit
main () =
  println (show_int (run_counter 1))
  println (with_name "Ada")
  let ints =
    handle both_ways (fn a b -> show_int (a + b)) with
      | ask () k -> resume k 21
  println ints
  let strings =
    handle both_ways (fn a b -> a ++ b) with
      | ask () k -> resume k "ab"
  println strings
  println (nested ())
```

期待する stdout:

```
20
hello Ada
42
abab
hello 7
```

`tests/ui/check-fail/effect_arguments.em`:

```haskell
-- E1015: an effect in a row takes as many type arguments as its declaration has. E2001: the type arguments of a
-- performed effect must match those in the row. E1008: the result of a `never` operation cannot be a type
-- parameter of its effect. E1003: the type parameters of an effect have distinct names.
effect State s where
  get : Unit -> s
  put : s -> Unit

effect Pair a a where
  first : Unit -> a

effect Fail e where
  never raise : Unit -> e

missing : Unit -> <State> Int
missing () = 0

extra : Unit -> <State Int String> Int
extra () = 0

wrong : Unit -> <State Int> Unit
wrong () = put "text"
```

期待する診断 (位置の順): E1003 (`a` is defined more than once、8:15)、E1008 (12:25、ラベル `this is a type parameter of the effect`)、E1015 (14:20、0 were given)、E1015 (17:18、2 were given)、E2001 (21:12、`put` performs `State String`, but the row allows `State Int`)。

- [ ] **Step 14: テストを通す**

Run: `cargo test`
Expected: `ui::run` と `ui::check_fail` が新しいスナップショットで失敗する。`cargo insta review` で、上の stdout と診断に一致することを確かめて承認し、もう一度 `cargo test` を通す。既存のスナップショットは変わらない。

- [ ] **Step 15: clippy と fmt を通してコミットする**

```bash
cargo clippy --all-targets && cargo fmt
git add -A
git commit -m "Type-check effect type arguments in row labels

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01WjhkcJTLXeazUACMXUrKfH"
```

---

### Task 3: `multi` の操作を HIR と型検査に通す

`multi` の E0004 を外し、`OpMultiplicity::Multi` を足す。`multi` の操作の節の `k` を `Unr` にし、扱うエフェクトに `multi` の操作がある handle の `return` の節が捕まえる変数に `Unr` の制約を付ける。実行は Task 4 で通す。

**Files:**
- Modify: `crates/eml_hir/src/hir.rs`、`crates/eml_hir/src/lower/effect.rs`、`crates/eml_hir/src/pretty.rs`
- Modify: `crates/eml_types/src/table/mod.rs`、`crates/eml_types/src/check/handle.rs`、`crates/eml_types/src/usage.rs`、`crates/eml_types/src/kind.rs`、`crates/eml_types/src/check/report.rs`
- Modify: `crates/eml_hir/tests/effects.rs` の `operation_signatures_are_checked` (種類1)
- Test: `crates/eml_types/tests/effects.rs`
- Create: `tests/ui/check-fail/multi_return_clause.em`

**Interfaces:**
- Consumes: Task 2 の `Rigids::with_effect_args`、`op_clause`
- Produces: `OpMultiplicity::Multi`、`KindReason::CapturedByReturnClause(String)`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/tests/effects.rs` の末尾に足す。

```rust
#[test]
fn a_continuation_of_a_multi_operation_may_be_resumed_twice() {
    let text = "effect Choice where\n  multi choose : Unit -> Bool\n\nboth : Unit -> Int\nboth () =\n  handle (if choose () then 1 else 2) with\n    | choose () k -> resume k True + resume k False";
    insta::assert_snapshot!(check_text(text), @r"
    choose : Unit -> <Choice> Bool
    both : Unit -> Int
      k#0 : Cont Bool Int <>
    ");
}

#[test]
fn the_return_clause_of_a_multi_handler_cannot_capture_a_linear_value() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\neffect Choice where\n  multi choose : Unit -> Bool\n\ncaptured : Unit -> Int\ncaptured () =\n  handle ask () with\n    | ask () k ->\n        handle (if choose () then 1 else 2) with\n          | choose () c -> resume c True + resume c False\n          | return n -> resume k n";
    insta::assert_snapshot!(check_text(text), @r"
    ask : Unit -> <Ask> Int
    choose : Unit -> <Choice> Bool
    captured : Unit -> Int
      k#0 : Cont Int Int <>
      c#1 : Cont Bool Int <>
      n#2 : Int
    ---
    E3001 10:14 `k` must be used exactly once, but the `return` clause of a handler with a `multi` operation captures it
      10:14 `k` is bound here
      note: linear values, such as the continuation of a `once` operation and closures that capture one, must be used exactly once
      note: the `return` clause runs each time a continuation of a `multi` operation is resumed
    ");
}
```

`crates/eml_hir/tests/effects.rs` の `operation_signatures_are_checked` の期待値を次にする (種類1。`multi` が HIR の表示に出て、E0004 が消える)。

```rust
    insta::assert_snapshot!(lower_text(text), @r"
    effect E
      with_row : Int -> <IO> Int
      constant : Int
      never bad : a -> a
      never good : Int -> b
      multi many : Unit -> Int
    ---
    E1007 2:21 an operation cannot have a row on its outermost arrows
    E1007 3:14 the signature of an operation must be a function type
    E1008 4:20 the result type of a `never` operation must be a type variable that does not appear in its parameters
    ");
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_hir --test effects operation_signatures_are_checked && cargo test -p eml_types --test effects`
Expected: FAIL (`multi` の E0004 と、`k` の E3001 が出る)

- [ ] **Step 3: HIR に `Multi` を足す**

`crates/eml_hir/src/hir.rs`:

```rust
/// 操作の多重度 (docs/spec/effects.md)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpMultiplicity {
    Never,
    Once,
    Multi,
}
```

`crates/eml_hir/src/lower/effect.rs` の `lower_operation` の多重度を次にする。

```rust
    let multiplicity = match decl.multiplicity() {
        Some(token) if token.kind() == SyntaxKind::NEVER_KW => OpMultiplicity::Never,
        Some(token) if token.kind() == SyntaxKind::MULTI_KW => OpMultiplicity::Multi,
        _ => OpMultiplicity::Once,
    };
```

`crates/eml_hir/src/pretty.rs` の多重度の表示に `OpMultiplicity::Multi => "multi "` を足し、変数名 `never` を `multiplicity` にする。

- [ ] **Step 4: 型検査で `multi` を扱う**

`crates/eml_types/src/table/mod.rs` の `Table::new` の多重度の対応に `OpMultiplicity::Multi => Multiplicity::Multi` を足す。

`crates/eml_types/src/check/handle.rs` の `op_clause` の継続の線形性:

```rust
        if let Some(k) = clause.k {
            // `multi` の操作の `k` は何度でも再開でき、捨ててもよい (docs/spec/effects.md の「継続の多重度と持ち越し規則」)
            let lin = match operation.multiplicity {
                OpMultiplicity::Multi => Linearity::Unr,
                OpMultiplicity::Once | OpMultiplicity::Never => Linearity::Lin,
            };
            let continuation = self.table.alloc(TyShape::Cont {
                arg: ty,
                lin: ArrowLin::Known(lin),
                row: outer.clone(),
                ret: result,
            });
            self.bind_pat(k, continuation);
        }
```

`op_clause` の doc コメントを「`once` の操作の `k` は `Lin`、`multi` の操作の `k` は `Unr` である」にする。`use eml_hir::{...}` に `OpMultiplicity` を足す。

`crates/eml_types/src/kind.rs` の `KindReason` に足す。

```rust
    /// 扱うエフェクトに `multi` の操作がある handler の、`return` の節が捕まえた変数。
    CapturedByReturnClause(String),
```

`crates/eml_types/src/usage.rs` の `ExprKind::Handle` の分岐:

```rust
            ExprKind::Handle {
                body: handled,
                effect,
                clauses,
                ret,
            } => {
                // 扱うエフェクトに `multi` の操作があれば、`k` を再開するたびに handler フレームを含む区間が写され、
                // `return` の節が何度も動きうる (docs/spec/linearity.md の「基本の規則」)
                let multi = effect.is_some_and(|effect| {
                    self.table.effect_multiplicity(effect) == Multiplicity::Multi
                });
                let mut uses = Uses::new();
                let inner = self.expr(*handled);
                let captured = self.captured_once(*handled, &[], inner);
                sequence(&mut uses, captured);
                if let Some(ret) = ret {
                    let inner = self.expr(ret.body);
                    let captured = self.captured_once(ret.body, &[ret.param], inner);
                    if multi {
                        let mut locals: Vec<LocalId> = captured.keys().copied().collect();
                        locals.sort();
                        for local in locals {
                            let name = body.locals[local].name.clone();
                            self.unr_local(local, KindReason::CapturedByReturnClause(name));
                        }
                    }
                    sequence(&mut uses, captured);
                }
                // (操作の節の扱いは今のまま)
```

`use crate::ty::{Linearity, Multiplicity};` にする。

`crates/eml_types/src/check/report.rs` の `linear_misuse` に足す。

```rust
        KindReason::CapturedByReturnClause(name) => (
            format!(
                "`{name}` must be used exactly once, but the `return` clause of a handler with a `multi` operation captures it"
            ),
            format!("`{name}` is bound here"),
        ),
```

note の `match` に足す。

```rust
        KindReason::CapturedByReturnClause(_) => {
            diagnostic = diagnostic.with_note(
                "the `return` clause runs each time a continuation of a `multi` operation is resumed",
            );
        }
```

- [ ] **Step 5: UI テストを足す**

`tests/ui/check-fail/multi_return_clause.em`:

```haskell
-- E3001: when the handled effect has a `multi` operation, the `return` clause may run more than once, so it cannot
-- capture a linear value such as the continuation of a `once` operation.
effect Ask where
  ask : Unit -> Int

effect Choice where
  multi choose : Unit -> Bool

pick : Unit -> <Choice> Int
pick () = if choose () then 1 else 2

captured : Unit -> Int
captured () =
  handle ask () with
    | ask () k ->
        handle pick () with
          | choose () c -> resume c True + resume c False
          | return n -> resume k n
```

期待する診断: E3001 の1件だけ (15:14、`` `k` is bound here ``、上の2つの note)。

- [ ] **Step 6: テストを通す**

Run: `cargo test`
Expected: `ui::check_fail` が `multi_return_clause.em` の新しいスナップショットで失敗する。`cargo insta review` で上の診断に一致することを確かめて承認し、もう一度 `cargo test` を通す。

- [ ] **Step 7: clippy と fmt を通してコミットする**

```bash
cargo clippy --all-targets && cargo fmt
git add -A
git commit -m "Type-check multi operations with unrestricted continuations

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01WjhkcJTLXeazUACMXUrKfH"
```

---

### Task 4: 共有された継続の区間を写して multi-shot で再開する

`Heap::take_or_copy` が共有された継続オブジェクトを受け取ったら、区間を先頭から切り離された handler フレームまで写す。元のフレームの参照の数は変えず、フレームをつねに一意に保つ。インタプリタの `resume` は `take_or_copy` を使う。

**Files:**
- Modify: `crates/eml_runtime/src/heap.rs`
- Modify: `crates/eml_interp/src/lib.rs`
- Create: `tests/ui/run/multi_resume.em`、`multi_choice.em`、`multi_captured.em`、`multi_loop.em`、`multi_over_once.em`

**Interfaces:**
- Consumes: Task 3 の `multi` の型検査 (UI テストのため)
- Produces: `HeapError::BrokenSegment`、`Heap::take_or_copy` の継続の写し (`copy_segment` は非公開)

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_runtime/src/heap.rs` の `mod tests` に足す。

```rust
    /// 区間の先頭から、切り離された handler フレームまでのフレーム。
    fn segment(heap: &Heap, top: ObjRef) -> Vec<ObjRef> {
        let mut frames = vec![top];
        loop {
            let next = match heap.get(*frames.last().unwrap()).unwrap() {
                Payload::Frame(Frame::Return { next, .. } | Frame::Apply { next, .. }) => *next,
                Payload::Frame(Frame::Handler {
                    next: Some(next), ..
                }) => *next,
                Payload::Frame(Frame::Handler { next: None, .. }) => return frames,
                other => panic!("not a frame: {other:?}"),
            };
            frames.push(next);
        }
    }

    #[test]
    fn take_or_copy_takes_a_unique_continuation_without_copying() {
        let mut heap = Heap::new();
        let detached = handler(&mut heap, vec![], None, None);
        let top = frame(&mut heap, vec![], detached);
        let k = heap.alloc(Payload::Continuation {
            top,
            handler: detached,
        });
        assert_eq!(
            heap.take_or_copy(k),
            Ok(Payload::Continuation {
                top,
                handler: detached
            })
        );
        heap.decref(top).unwrap();
        assert!(heap.live_objects().is_empty());
    }

    #[test]
    fn take_or_copy_copies_the_segment_of_a_shared_continuation() {
        let mut heap = Heap::new();
        let s = string(&mut heap, "saved");
        let clause = heap.alloc(Payload::Closure(Closure {
            function: 0,
            args: vec![],
        }));
        let detached = handler(&mut heap, vec![Value::Obj(clause)], None, None);
        let below_attached = frame(&mut heap, vec![], detached);
        // 本体の中の別の handle は、外側につながったまま区間に入る
        let attached = handler(&mut heap, vec![], None, Some(below_attached));
        let top = frame(&mut heap, vec![(0, Value::Obj(s))], attached);
        let k = heap.alloc(Payload::Continuation {
            top,
            handler: detached,
        });
        heap.dup(k).unwrap();
        let Payload::Continuation {
            top: copied_top,
            handler: copied_handler,
        } = heap.take_or_copy(k).unwrap()
        else {
            panic!("not a continuation");
        };
        let original = segment(&heap, top);
        let copied = segment(&heap, copied_top);
        assert_eq!(original, [top, attached, below_attached, detached]);
        assert_eq!(copied.len(), 4);
        assert_eq!(copied[3], copied_handler);
        assert!(copied.iter().all(|frame| !original.contains(frame)));
        // どちらの区間のフレームも一意である
        for &frame in original.iter().chain(&copied) {
            assert!(heap.is_unique(frame).unwrap());
        }
        // 退避した文字列と節のクロージャは、両方の区間から参照される
        assert!(!heap.is_unique(s).unwrap());
        assert!(!heap.is_unique(clause).unwrap());
        let copy = heap.alloc(Payload::Continuation {
            top: copied_top,
            handler: copied_handler,
        });
        heap.decref(copy).unwrap();
        heap.decref(k).unwrap();
        assert!(heap.live_objects().is_empty());
    }

    #[test]
    fn copying_a_long_segment_does_not_overflow_the_stack() {
        let mut heap = Heap::new();
        let detached = handler(&mut heap, vec![], None, None);
        let mut top = detached;
        for _ in 0..200_000 {
            top = frame(&mut heap, vec![], top);
        }
        let k = heap.alloc(Payload::Continuation {
            top,
            handler: detached,
        });
        heap.dup(k).unwrap();
        let copy = heap.take_or_copy(k).unwrap();
        let copy = heap.alloc(copy);
        heap.decref(copy).unwrap();
        heap.decref(k).unwrap();
        assert!(heap.live_objects().is_empty());
    }
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_runtime`
Expected: `take_or_copy_copies_the_segment_of_a_shared_continuation` が FAIL (今の `copy` は `top` を共有させるので、写した区間の先頭が元と同じになる)

- [ ] **Step 3: 区間の写しを実装する**

`crates/eml_runtime/src/heap.rs`:

`HeapError` に足し、`Display` に文言を足す。

```rust
    /// 継続の区間が、切り離された handler フレームで終わっていない。
    BrokenSegment,
```

```rust
            HeapError::BrokenSegment => {
                f.write_str("a continuation does not end at a detached handler")
            }
```

`take_or_copy` を次にする。

```rust
    /// オブジェクトの所有権を受け取って中身を使う側のための手続き。一意なら解放して中身を返す。共有されていれば
    /// 中身を写し、写した中身の子の参照を1つずつ増やしてから、元の参照を1つ手放す。子は解放と同じ `children` で
    /// 数えるので、写すときと解放するときで数える参照が一致する。継続オブジェクトは区間のフレームごと写す。フレームを
    /// 共有させないためである (docs/spec/runtime.md)。
    pub fn take_or_copy(&mut self, obj: ObjRef) -> Result<Payload, HeapError> {
        if self.is_unique(obj)? {
            return self.take(obj);
        }
        let segment = match &self.object(obj)?.payload {
            Payload::Continuation { top, .. } => Some(*top),
            _ => None,
        };
        let copy = match segment {
            Some(top) => {
                let (top, handler) = self.copy_segment(top)?;
                Payload::Continuation { top, handler }
            }
            None => {
                let copy = copy(&self.object(obj)?.payload);
                let mut shared = Vec::new();
                children(&copy, &mut shared);
                for child in shared {
                    self.dup(child)?;
                }
                copy
            }
        };
        self.decref(obj)?;
        Ok(copy)
    }

    /// 継続の区間を、先頭のフレームから切り離された handler フレーム (`next` が `None`) まで写し、写した区間の先頭と
    /// handler フレームを返す。写したフレームは `next` 以外の子の参照を1つずつ増やし、`next` は写した次のフレームを
    /// 指す。元のフレームの参照の数は変えないので、写した後も両方の区間のフレームは一意である。長い区間で Rust の
    /// スタックを溢れさせないよう、ループでたどる (docs/spec/runtime.md)。
    fn copy_segment(&mut self, top: ObjRef) -> Result<(ObjRef, ObjRef), HeapError> {
        let mut frames = Vec::new();
        let mut current = top;
        loop {
            frames.push(current);
            current = match &self.object(current)?.payload {
                Payload::Frame(Frame::Handler { next: None, .. }) => break,
                Payload::Frame(
                    Frame::Return { next, .. }
                    | Frame::Apply { next, .. }
                    | Frame::Handler {
                        next: Some(next), ..
                    },
                ) => *next,
                _ => return Err(HeapError::BrokenSegment),
            };
        }
        // 下から写し、写した次のフレームを `next` に入れる
        let mut below = None;
        let mut handler = None;
        for &frame in frames.iter().rev() {
            let mut payload = copy(&self.object(frame)?.payload);
            set_next(&mut payload, below)?;
            let mut shared = Vec::new();
            children(&payload, &mut shared);
            // 写した次のフレームは、この写しだけが所有する
            for child in shared.into_iter().filter(|&child| Some(child) != below) {
                self.dup(child)?;
            }
            let copied = self.alloc(payload);
            handler.get_or_insert(copied);
            below = Some(copied);
        }
        match (below, handler) {
            (Some(top), Some(handler)) => Ok((top, handler)),
            _ => Err(HeapError::BrokenSegment),
        }
    }
```

`copy` の継続の分岐は、`copy_segment` が扱うので通らない。

```rust
        Payload::Continuation { .. } => {
            unreachable!("a shared continuation is copied with its segment by `copy_segment`")
        }
```

`copy` の doc コメントを「中身の写し。子の参照は数え直さないので、`take_or_copy` と `copy_segment` だけが使う。継続オブジェクトは区間ごと写すので、ここでは扱わない。」にする。ファイルの `copy` の後に足す。

```rust
/// 写したフレームの次を、写した次のフレームにする。切り離された handler フレームだけが次を持たない。
fn set_next(payload: &mut Payload, below: Option<ObjRef>) -> Result<(), HeapError> {
    match payload {
        Payload::Frame(Frame::Return { next, .. } | Frame::Apply { next, .. }) => {
            *next = below.ok_or(HeapError::BrokenSegment)?;
        }
        Payload::Frame(Frame::Handler { next, .. }) => *next = below,
        _ => return Err(HeapError::BrokenSegment),
    }
    Ok(())
}
```

`Payload::Continuation` の doc コメントに「共有された継続を写すときは、区間のフレームごと写す (`take_or_copy`)」を足す。

- [ ] **Step 4: ランタイムのテストを通す**

Run: `cargo test -p eml_runtime`
Expected: PASS

- [ ] **Step 5: インタプリタの `resume` を `take_or_copy` にする**

`crates/eml_interp/src/lib.rs` の `resume`:

```rust
    /// 継続オブジェクトの handler フレームの外側に今の継続をつなぎ、先頭のフレームに値を返す。末尾でない `resume`
    /// では、その前に呼び出しのフレームが積まれている。`multi` の継続をもう一度使うなら継続は共有されていて、
    /// `take_or_copy` が区間を写す。どちらの場合も区間のフレームは一意なので、handler フレームを書き換えてよい
    /// (docs/spec/core-ir.md)。
    fn resume(&mut self, k: Value, value: Value) -> Result<Step, Fault> {
        let Value::Obj(obj) = k else {
            return Err(Fault::Internal(
                "resuming a value that is not a continuation",
            ));
        };
        let Payload::Continuation { top, handler } =
            self.heap.take_or_copy(obj).map_err(Fault::Heap)?
        else {
            return Err(Fault::Internal(
                "resuming an object that is not a continuation",
            ));
        };
        // (以降は今のまま)
```

`ret` のコメント「`once` の継続までは継続を複製しないので、フレームは常に一意である。共有されたフレームは段階3b の `multi` で扱う」を「フレームはつねに一意である。共有されうるのは継続オブジェクトだけで、再開するときに区間を写す (docs/spec/runtime.md)」にする。

- [ ] **Step 6: UI テストを足す**

`tests/ui/run/multi_resume.em`:

```haskell
-- `multi` operations may be resumed any number of times. A `resume` that is not the last use of the continuation
-- copies the captured frames, so each resumption runs the rest of the computation on its own. Dropping the
-- continuation or not using it releases the frames and the strings they saved.
effect Choice where
  multi choose : Unit -> Bool

pick : Unit -> <Choice> String
pick () =
  let prefix = "picked "
  let n = if choose () then 1 else 2
  prefix ++ show_int n

both : Unit -> String
both () =
  handle pick () with
    | choose () k -> resume k True ++ " and " ++ resume k False

first_only : Unit -> String
first_only () =
  handle pick () with
    | choose () k -> resume k True

dropped : Unit -> String
dropped () =
  handle pick () with
    | choose () k ->
        drop k
        "dropped"

unused : Unit -> String
unused () =
  handle pick () with
    | choose () k -> "unused"

counted : Unit -> String
counted () =
  handle pick () with
    | choose () k -> resume k False ++ "/" ++ resume k True
    | return s -> "<" ++ s ++ ">"

main : Unit -> <IO> Unit
main () =
  println (both ())
  println (first_only ())
  println (dropped ())
  println (unused ())
  println (counted ())
```

期待する stdout:

```
picked 1 and picked 2
picked 1
dropped
unused
<picked 2>/<picked 1>
```

`tests/ui/run/multi_choice.em`:

```haskell
-- Every combination of three choices runs. The copied part of the continuation holds a saved string and the
-- handler of another effect that is still attached to it, and each copy releases what it holds.
effect Choice where
  multi choose : Unit -> Bool

effect Name where
  name : Unit -> String

bit : Bool -> Int
bit b = if b then 1 else 0

describe : Unit -> <Choice, Name> String
describe () =
  let prefix = name ()
  let a = bit (choose ())
  let b = bit (choose ())
  let c = bit (choose ())
  prefix ++ ":" ++ show_int (a * 4 + b * 2 + c)

labelled : Unit -> <Choice> String
labelled () =
  handle describe () with
    | name () k -> resume k "bits"

every : Unit -> String
every () =
  handle labelled () with
    | choose () k -> resume k False ++ " " ++ resume k True

paths : Unit -> Int
paths () =
  handle labelled () with
    | choose () k -> resume k False + resume k True
    | return s -> 1

main : Unit -> <IO> Unit
main () =
  println (every ())
  println (show_int (paths ()))
```

期待する stdout:

```
bits:0 bits:1 bits:2 bits:3 bits:4 bits:5 bits:6 bits:7
8
```

`tests/ui/run/multi_captured.em`:

```haskell
-- The continuation of a `multi` operation is an unrestricted value. A closure can capture it and be called more
-- than once, and a local function can take it and resume it more than once.
effect Ask where
  multi ask : Unit -> Int

plus_one : Unit -> <Ask> Int
plus_one () = ask () + 1

twice : (Int -> <e> Int) -> <e> Int
twice f = f 10 + f 20

main : Unit -> <IO> Unit
main () =
  let captured =
    handle plus_one () with
      | ask () k -> twice (fn n -> resume k n)
  println (show_int captured)
  let passed =
    handle plus_one () with
      | ask () k ->
          let go = fn cont value -> resume cont value
          go k 1 * go k 2
  println (show_int passed)
```

期待する stdout:

```
32
6
```

`tests/ui/run/multi_loop.em`:

```haskell
-- Each level resumes the continuation twice, so the first resumption copies the captured frames. Ten thousand
-- copies run and are released without leaks.
effect Choice where
  multi choose : Unit -> Bool

loop : Int -> Int -> <Choice> Int
loop n acc = if n == 0 then acc else if choose () then loop (n - 1) (acc + 1) else acc

main : Unit -> <IO> Unit
main () =
  let total =
    handle loop 10000 0 with
      | choose () k -> resume k False + resume k True
  println (show_int total)
```

期待する stdout (`0 + 1 + ... + 9999 + 10000`):

```
50005000
```

`tests/ui/run/multi_over_once.em`:

```haskell
-- The continuation `k` of a `once` operation is saved across a `multi` operation, so resuming the `multi`
-- continuation twice copies `k` and resumes it on each path. The carry-over rule of stage 5 will reject this
-- program statically; until then the runtime copies `k` with its frames and releases every copy.
effect Ask where
  ask : Unit -> Int

effect Choice where
  multi choose : Unit -> Bool

inner : Unit -> <Choice> Int
inner () =
  handle ask () with
    | ask () k ->
        let b = choose ()
        resume k (if b then 1 else 2)

main : Unit -> <IO> Unit
main () =
  let total =
    handle inner () with
      | choose () c -> resume c True + resume c False
  println (show_int total)
```

期待する stdout:

```
3
```

- [ ] **Step 7: テストを通す**

Run: `cargo test`
Expected: `ui::run` が5つの新しいスナップショットで失敗する。`cargo insta review` で、上の stdout に一致し、stderr が空であることを確かめて承認し、もう一度 `cargo test` を通す。`debug_heap` が有効なので、リークがあれば `Leak` で失敗する。

- [ ] **Step 8: clippy と fmt を通してコミットする**

```bash
cargo clippy --all-targets && cargo fmt
git add -A
git commit -m "Copy the segment of a shared continuation to resume it more than once

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01WjhkcJTLXeazUACMXUrKfH"
```

---

### Task 5: 文書を直す

段階3b の決定を `docs/spec/` と `docs/implementation/` に反映する。日本語を書く前に `yomiyasu:yomiyasu` スキルを読み、既存の文書の文体 (常体、1文に1つのこと、英単語の前後の空白) に合わせる。

**Files:**
- Modify: `docs/spec/effects.md`、`declarations.md`、`types.md`、`linearity.md`、`core-ir.md`、`runtime.md`、`diagnostics.md`
- Modify: `docs/implementation/architecture.md`、`status.md`、`testing.md`

- [ ] **Step 1: spec を直す**

`docs/spec/effects.md`:
- 「エフェクトの宣言と操作」の多重度の項の後に足す: 「エフェクトは型引数を持てる (`effect State s`)。row では `<State Int | e>` のように、宣言と同じ個数の型引数を付けて書く。個数が違えば E1015 にする。操作のシグネチャで、エフェクトの型引数と同じ名前の型変数はその型引数を指す。ほかの型変数は、操作ごとに暗黙に量化する。」
- `never` の項の末尾に足す: 「エフェクトの型引数は、呼び出した側が自由に選べないので、`never` の操作の結果の型に使えない (E1008)。」
- 「継続の多重度と持ち越し規則」の表の後に足す: 「`multi` の操作の `k` は何度でも再開でき、`drop k` しても使わなくてもよい。共有された継続を再開するときは、捕まえた区間を写してから再開する ([Core IR とインタプリタ](core-ir.md))。」
- 「handler の意味」の「操作の節は、handler が生きている間…」の項の末尾に足す: 「ただし、扱うエフェクトに `multi` の操作があれば、`k` を再開するたびに handler フレームを含む区間が写され、`return` の節も何度も動きうる。そのため、`return` の節が捕まえる変数にも `Unr` の制約が付く。」
- 同じ節の「操作の引数の型に現れる Kind 変数…」の項の末尾に足す: 「エフェクトの型引数の Kind 変数は固定しない。handle ごとに具体的な型で節を検査するので、節での使い方がその型の Kind に伝わる。」
- 同じ節に項を足す: 「handle は、扱うエフェクトの型引数を handle ごとの新しい推論用の変数にし、本体を `<E α1 .. αn | ρ>` で検査する。節では、エフェクトの型引数をその変数で、操作自身の型変数を節だけの rigid な変数で具体化する。」

`docs/spec/declarations.md` の「`effect`」:
- 多重度の項の後に足す: 「エフェクトは型引数を持てる (`effect State s`)。型引数は型だけで、row を引数に取るエフェクトはまだ書けない。型引数の名前が重複したら E1003 にする」「操作のシグネチャで、エフェクトの型引数と同じ名前の型変数はその型引数を指す。ほかの型変数は、操作ごとに暗黙に量化する」「row の中のエフェクトには、宣言と同じ個数の型引数を付ける (`<State Int>`)。個数が違えば E1015 にする」
- `never` の項を「`never` の操作の結果の型は、宣言で自由な型変数として書く (`never fail : String -> a`)。結果の型は操作自身の型変数で、引数に現れてはならない。エフェクトの型引数は結果の型に使えない (E1008)」にする

`docs/spec/types.md` の「関数型」の row の書き方の項の後に足す: 「row のラベルは、エフェクトとその型引数である (`<State Int | e>`)。scoped labels の単一化では、同じエフェクトのラベルを、それぞれの row の中の順で対にし、対にしたラベルの型引数を単一化する。型引数が一致しなければ E2001 にする。」

`docs/spec/linearity.md` の「基本の規則」:
- handle の本体と `return` の節の項の末尾に足す: 「扱うエフェクトに `multi` の操作があれば、`return` の節は何度も動きうるので、`return` の節が捕まえる変数にも、使用の回数によらず `Unr` の制約を加える ([エフェクトと handler](effects.md) の「handler の意味」)。」
- 操作の引数の型の Kind 変数の項の末尾に足す: 「エフェクトの型引数には加えない。handle ごとに具体的な型で節を検査するためである。」

`docs/spec/core-ir.md` の `resume k v` の項を次にする: 「`resume k v` は、継続オブジェクトの handler フレームの次に今の継続をつなぎ、区間の先頭に `v` を返す。区間のフレームはつねに一意なので、つなぎ直しは書き換え1回で済む。一意なオブジェクトの書き換えは観測できないので、フレームをイミュータブルとして扱う前提と両立する (Perceus の reuse と同じ理屈)。継続オブジェクトが共有されていれば (`multi` の `k` をもう一度使う場合)、区間のフレームを写してから、写した handler フレームの次に今の継続をつなぐ。元の区間はそのまま残るので、継続を何度でも再開できる。最後の1回の再開では継続が一意なので、写さずに書き換える。」

`docs/spec/runtime.md` の「ランタイムの API」:
- 「共有されたオブジェクトの中身を使う側 (クロージャの呼び出し、段階3b の multi-shot の再開) は…」の括弧を「(クロージャの呼び出しと、継続の再開)」にする
- 項を足す: 「フレームはつねに一意である。共有されうるのは継続オブジェクトだけで、共有された継続オブジェクトの複製は、区間のフレームを先頭から切り離された handler フレームまで写す。写したフレームは `next` 以外の子の参照を1つずつ増やし、`next` は写した次のフレームを指す。元のフレームの参照の数は変えないので、写した後も両方の区間のフレームは一意である。区間はループでたどる。」

`docs/spec/diagnostics.md`:
- 「割り当て済みの番号」の E1014 の行の後に足す: `| E1015 | \`TYPE_ARGUMENT_COUNT\` | row の中のエフェクトの型引数の個数が宣言と違う。段階4の \`data\` の型引数でも使う |`
- E2001 の行の説明の末尾に足す: 「呼び出しの row のエフェクトの型引数が今の row と一致しないときも E2001 にし、呼び出しを primary にする」
- E3001 の行の説明の括弧を「(`once` の操作の `k` と、それを捕まえたクロージャ)」のまま残し、末尾に「`multi` の操作を持つ handler の `return` の節が捕まえた場合を含む」を足す

- [ ] **Step 2: architecture.md を直す**

`docs/implementation/architecture.md`:
- 「`eml_hir` の内部」の型とエフェクトの item の項の末尾に足す: 「エフェクトの宣言は型引数を `EffectDef::generics` に持つ。操作の `Generics` は、エフェクトの型引数を先頭に写して始め、その個数を `Operation::effect_params` に持つ。row のエフェクトは `EffectRef` (エフェクトと型引数) で、型引数の個数は `ItemScope::effect_params` で確かめる (E1015)」
- 「`eml_types` の内部」の「型構成子は `TyShape::Con(TypeDefId)`、row のラベルは `EffectId` である。…」の項を「型構成子は `TyShape::Con(TypeDefId)`、row のラベルは `Label` (エフェクトの ID と型引数) である。外に出す型は `Type::Con { id, name }` と `EffectLabel { id, name, args }` で、…」にし、項を足す: 「row の単一化は、同じエフェクトのラベルを row の中の順で対にし、型引数を単一化する。一致しなければ `UnifyError::EffectArgs` で、`include_call_row` が E2001 にする」
- 継続の型の項の「`once` の操作の `k` の線形性は `Lin` である」を「`once` の操作の `k` の線形性は `Lin`、`multi` の操作の `k` は `Unr` である」にする
- 操作のスキームの項の末尾に足す: 「row のラベルの型引数は、エフェクトの型引数の rigid 変数である。操作の引数の型の Kind 変数を `Unr` に固定する `Table::unrestricted` は、エフェクトの型引数の Kind 変数を外す」
- handle の検査の項の「節では操作の型変数を新しい rigid 変数にする」を「handle ごとにエフェクトの型引数を新しい推論用の変数にし、節ではエフェクトの型引数をその変数に、操作自身の型変数だけを新しい rigid 変数にする (`Rigids::with_effect_args`)」にする
- SCC と使用回数のパスの項の末尾に足す: 「使用回数のパスは、扱うエフェクトに `multi` の操作がある handle の `return` の節が捕まえる変数にも `Unr` の制約を出す」
- 「`eml_core_ir`、`eml_runtime`、`eml_interp` の内部」の最後の項の末尾に足す: 「共有された継続の `resume` は `Heap::take_or_copy` で区間を写す (`copy_segment`)。フレームはつねに一意で、共有されうるのは継続オブジェクトだけである」

- [ ] **Step 3: status.md と testing.md を直す**

`docs/implementation/status.md`:
- 「名前解決以降の実装段階」の表の 3b の状態を「完了」にする
- 「R2b-1 HIR の構造」の段落の「ただし、エフェクトの宣言の `Generics` は、エフェクトの型引数と一緒に段階3b で入れる (「次の作業の注意点」)。」を「エフェクトの宣言の `Generics` は、エフェクトの型引数と一緒に段階3b で入れた。」にする
- 「各 crate の実装状況」: `eml_hir` を「段階3b まで実装済み。…」にし、末尾に「エフェクトの型引数と E1015、`multi` の操作」を足す。`eml_types` を「段階3b まで実装済み。…」にし、末尾に「row のラベルの型引数、`multi` の操作の `k`、`return` の節の捕獲の制約」を足す。`eml_core_ir` を「段階3b まで実装済み。…」にする (3b の変更はない)。`eml_runtime` を「段階3b まで実装済み。…」にし、末尾に「共有された継続の区間の複製」を足す。`eml_interp` を「段階3b まで実装済み。…」にし、末尾に「multi-shot の再開」を足す
- 「次の作業の注意点」:
  - 段階4の最初の項の「引数を持つ `data` は段階4で、引数を持つ `effect` は段階3b で、引数の単一化と一緒に入れる」を「引数を持つ `data` は段階4で、引数の単一化と一緒に入れる。row のラベルの型引数 (段階3b) と同じく、型の表の形に引数を持たせる」にする
  - 「段階3b: multi-shot の `resume` では…」「段階3b: `multi` の操作の節の `k` は `Unr` にする…」「段階3b: multi-shot では、`return` の節が2回以上動きうる…」の3項を削る
  - 「段階3b と5: row 変数の多重度 `σ` は…」を「段階5: row 変数の多重度 `σ` は、スキームの多相化と具体化で制約を複製しているが、上限の制約が出ないので確かめていない。段階3b の `multi` でも下限しか出ない。持ち越し規則で上限が出たときにテストを足す」にする
  - 項を足す: 「段階5: 持ち越し規則がないので、`multi` の呼び出しをまたいで生きている `once` の `k` は、区間を写すときに複製され、2回再開されうる (`tests/ui/run/multi_over_once.em`)。ランタイムは共有された継続を写して再開するのでメモリ安全で、`debug_heap` も通る。持ち越し規則で静的に止め、このテストを `check-fail/` に移す」
  - 項を足す: 「段階5: 外側の `multi` の handler が、内側の handler フレームを含む区間を写すと、内側の `return` の節が2回動く。`return` の節が捕まえた値は、本体の呼び出しをまたいで生きている値に当たるので、持ち越し規則で扱う」
- 「完了した作業」の表の最後に足す: `| 縦の貫通 段階3b | \`multi\` の操作と multi-shot の再開、エフェクトの型引数を通した。row のラベルに型引数を持たせ、同じエフェクトのラベルを順に対にして単一化する。共有された継続を再開するときは区間を写し、フレームはつねに一意に保つ。持ち越し規則は段階5に残した |`

`docs/implementation/testing.md` の「テストの変更の記録」の最後に足す。

```markdown
### 縦の貫通 段階3b

- `multi` の操作とエフェクトの型引数を通すようになったので、`tests/ui/check-fail/later_stage_effects.em` から `multi` とエフェクトの型引数の部分を除き、`from` の E0004 だけを確かめるようにした (種類1)。`multi` の操作は `run/multi_*.em` と `check-fail/multi_return_clause.em`、エフェクトの型引数は `run/effect_parameters.em` と `check-fail/effect_arguments.em` で確かめる
- `eml_hir/tests/effects.rs` の `operation_signatures_are_checked` の期待値から `multi` の E0004 が消え、HIR の表示が `multi many : Unit -> Int` になった (種類1)。`effect_type_parameters_and_arguments_come_in_stage_3b` は、型引数が HIR に入ることを確かめる3つのテスト (`effects_take_type_parameters_and_rows_take_type_arguments`、`operations_see_the_type_parameters_of_their_effect_first`、`type_arguments_and_parameters_of_effects_are_checked`) に置き換えた (種類1)
- `eml_types` の `table/tests.rs` と `ty.rs` の単体テストの row とラベルの組み立てを `Label` と `EffectLabel::args` に合わせ、`EffectDef` の組み立てに `generics` を足した (種類3)。期待値は変えていない
- `tests/ui/run/multi_over_once.em` は、持ち越し規則がない段階3b での振る舞い (メモリ安全に `once` の `k` を写す) を確かめる。段階5で持ち越し規則を入れたら `check-fail/` に移す (種類1の予定)
```

- [ ] **Step 4: 文書の検査を通してコミットする**

Run: `python3 ~/.claude/plugins/cache/yomiyasu/yomiyasu/1.0.4/skills/yomiyasu/scripts/yomiyasu_lint.py docs/spec/effects.md` (直した文書ごとに実行し、`bold_not_rendered` だけは必ず直す。英単語の前後の空白と箇条書きの比率の指摘は、リポジトリの文書の書き方なので残す)

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: PASS

```bash
git add -A
git commit -m "Document stage 3b in the specs, architecture, and status

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01WjhkcJTLXeazUACMXUrKfH"
```
