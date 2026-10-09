# S4b 単相化 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 関数のコードを、入口から届く (関数, 型引数) の instance ごとに変換する。データの配置と row は一様のまま変えない。

**Architecture:** Task 1 で `eml_types` に `OpVar`、`Substitution`、`display_bounded` を足す (出力は変わらない)。Task 2 で Core IR のテキストの形が引用符で囲んだ関数の名前を読み書きできるようにする。Task 3 で translate の `reachable` と関数の番号の表を、instance の表 (`translate/instances.rs`) に置き換える。この時点では鍵に型引数を入れず、出力を1バイトも変えない。Task 4 で鍵に型引数を入れ、多相再帰の検出、本体の型への代入、instance の名前を入れる (ここで期待値が変わる)。Task 5 でコンパイル時間のテストと S4b の記録を足し、Task 6 で文書を段の終わりの形にする。

**Tech Stack:** Rust (edition 2024)、insta、eml の UI テスト、`bench/run.sh`。

**Spec:** `docs/superpowers/specs/2026-10-10-s4b-monomorphization-design.md`。spec とこの計画は、Task 1 の前に main にコミットしてある。各タスクのコミットは、そのタスクのファイルだけを名前で `git add` する。

| 見出し | 中身 | 出力 |
|---|---|---|
| Task 1 | `OpVar`、型変数を含むかの印、`Substitution`、`display_bounded` | 変わらない |
| Task 2 | テキストの形の引用符で囲む関数の名前 | 変わらない |
| Task 3 | instance の表への置き換え (鍵は型引数なし) | 変わらない |
| Task 4 | 型引数の鍵、多相再帰、本体の型への代入、名前、期待値の変更 | 変わる |
| Task 5 | コンパイル時間のテスト、`bench/run.sh` の S4b の記録 | 変わらない |
| Task 6 | 文書 | なし |

## Global Constraints

- 各タスクの終わりに、`cargo test` がすべて通り、`cargo clippy --all-targets` が警告を出さず、`cargo fmt --check` が差分を出さない。`cargo test -p eml_cli --test integration citations` も通る
- UI テストの出力 (`crates/eml_cli/tests/snapshots/` の既存のスナップショット) は変えない。足してよい UI テストは `tests/ui/run/functions/polymorphic_recursion.em` だけである
- テストの変更は、spec の「テストの変更」に書いた範囲の中だけで行う。Task 1、2、3、5 は既存のテストの期待値を変えない (Task 1 はコメントと網羅的な `match` の腕だけ、Task 2 はテストの補助関数だけを直す)。スナップショットは、変わった中身を読んでから受け入れる
- 名前 (spec のとおり)
  - `eml_types`: `TypeKind::OpVar(String)`、`Substitution`、`Substitution::new`、`TypeStore::substitute`、`TypeStore::contains_type_vars`、`TypeStore::display_bounded`
  - `eml_core_ir`: `translate/instances.rs`、`InstanceId`、`Instance`、`Instances`、`instances::collect`
  - instance の名前 `f@[Int, String]`、一様な位置 `_`、長い名前の代わり `f@3`、テキストの形の `fn "f@[Int]"(…)` と `&"f@[Int]"`
- 日本語のコメントと文書は、書く前に `yomiyasu:yomiyasu` のスキルを呼び、その規則に従う。コメントは「なぜ」を書き、規則を指すときは `docs/` のパスを引く。文書の見出しを「」で引くのは、その見出しが存在してからにする (citations のテストが確かめる)。CLAUDE.md は英語で書く。UI テストの `.em` のコメントは、既存の UI テストに合わせて英語で書く
- バックエンドの新しいコード (instance を集める手順、強連結成分) は、グラフの深さに比例して Rust のスタックを使わない。型をたどる再帰 (`substitute`、型変数の出現) は、既存の型のたどり方と同じく型の深さまで進んでよい (status.md の「深さと性能」に既にある制限の範囲)
- コミットメッセージの末尾には次の2行を付ける。期待値を変えたコミットは、変えた範囲と理由を本文に書く

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01FBntPy6jdwdA3kbyvHpkgQ
  ```

- `git diff` は外部の差分ツールを使う設定なので、スクリプトでは `git diff --no-ext-diff` を使う
- 同じワークスペースの2つの木で1つの `CARGO_TARGET_DIR` を共有しない

## Review Focus

- 部分を共有する型 (`let f1 = f0 ident` の列) を含むプログラムで、名前や代入や型変数の出現のたどりが指数の時間になる (Task 4 の `a_long_shared_type_argument_gets_an_ordinal_name`、Task 5 の時間のテスト)
- 多相再帰が、自分自身への辺だけの1つの節点や、相互再帰や、関数を値として渡す形で見逃され、instance を集める手順が止まらない (Task 4 の `depth`、相互再帰、値として渡すテスト)
- 節の型変数が handle の結果として節の外へ出て、そこで総称な関数に渡される (Task 4 の `a_clause_variable_that_escapes_through_the_handle_is_uniform`)
- 型変数を持たない関数しかないプログラムで、関数の並びや名前が変わる (Task 3 は出力が1バイトも変わらないこと、Task 4 の `monomorphic_programs_keep_their_function_order_and_names`)
- 入口に型変数を持つ関数を選ぶ (Task 4 の `a_polymorphic_entry_is_uniform`)

---

### Task 1: `OpVar`、`Substitution`、`display_bounded`

**Files:**
- Modify: `crates/eml_types/src/store.rs`
- Modify: `crates/eml_types/src/lib.rs` (`Substitution` の再公開)
- Modify: `crates/eml_types/src/table/mod.rs` (`RigidInfo`、`fresh_operation_rigid`)
- Modify: `crates/eml_types/src/table/export.rs`
- Modify: `crates/eml_types/src/shape.rs` (`instantiate_with_effect_args`)
- Modify: `crates/eml_core_ir/src/translate/types.rs` (`repr` の腕)
- Modify: `crates/eml_core_ir/tests/externs.rs` (`has_type_var` の腕)
- Test: `crates/eml_types/src/store.rs` の `#[cfg(test)] mod tests`、`crates/eml_types/tests/instantiations.rs`

**Interfaces:**
- Produces:
  - `TypeKind::OpVar(String)`
  - `pub struct Substitution`、`Substitution::new(vars: impl IntoIterator<Item = (String, TypeId)>) -> Substitution`
  - `TypeStore::substitute(&mut self, ty: TypeId, subst: &mut Substitution) -> TypeId`
  - `TypeStore::contains_type_vars(&self, id: TypeId) -> bool`
  - `TypeStore::display_bounded(&self, id: TypeId, names: &DisplayNames, limit: usize) -> Option<String>` (`limit` は文字の数)

- [ ] **Step 1: 書き出しの失敗するテストを書く**

`crates/eml_types/tests/instantiations.rs` の `a_clause_variable_is_shown_like_the_function_variable_of_the_same_name` の直後に足す。ファイルの先頭の `use` に `eml_types::TypeKind` を足す。

```rust
#[test]
fn a_clause_variable_is_a_different_type_from_the_function_variable_of_the_same_name() {
    // 単相化は関数の型変数にだけ代入するので、節の操作ごとの型変数は別の種類で書き出す
    // (docs/implementation/architecture.md の「`eml_types` の内部」)
    let text = format!(
        "{ID}effect Pick where\n  pick : a -> a\n\nrun : a -> a\nrun v =\n  handle id v with\n    | pick x k -> k (id x)"
    );
    let checked = check(&text);
    let program = &checked.program;
    let (id, _) = program
        .functions()
        .find(|(_, function)| function.name == "run")
        .unwrap();
    let args: Vec<TypeKind> = checked.typed.bodies[id]
        .instantiations
        .iter()
        .map(|(_, instantiation)| checked.typed.types.kind(instantiation.args[0]).clone())
        .collect();
    assert_eq!(
        args,
        [
            TypeKind::Rigid("a".to_string()),
            TypeKind::OpVar("a".to_string())
        ]
    );
}
```

`ID` はこのファイルにすでにある定数である。ほかの参照 (`handle` の中の `pick` は記録されない) があれば並びが変わるので、そのときは `id` の2つの参照だけを取り出すように直す。

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_types --test integration instantiations::a_clause_variable_is_a_different`
Expected: コンパイルエラー (`no variant named OpVar`)

- [ ] **Step 3: `OpVar` と書き出しを入れる**

`store.rs` の `TypeKind` の `Rigid` の後に足す。

```rust
    /// handler の節で、操作ごとに量化した型変数。節は操作がどの型で呼ばれても動くので、単相化でも一様に扱う。関数の
    /// 型変数と名前が同じでも別の型である (docs/implementation/architecture.md の「`eml_types` の内部」)。
    OpVar(String),
```

`Rigid` の doc コメントを「関数のシグネチャの型変数。名前で登録するので、同じ名前の変数は同じ型になる。」にする。`for_each_child` の最後の腕を `TypeKind::Rigid(_) | TypeKind::OpVar(_) | TypeKind::Flexible | TypeKind::Error => {}` にし、`TypeDisplay` の `TypeKind::Rigid(name) => f.write_str(name),` を `TypeKind::Rigid(name) | TypeKind::OpVar(name) => f.write_str(name),` にする。

`table/mod.rs` の `RigidInfo` に印を足す。

```rust
struct RigidInfo {
    name: String,
    /// 型変数の Kind `Type<μ>` の `μ`。
    linearity: KindVar,
    /// handler の節で、操作ごとに量化した型変数を具体化した変数か。書き出しで `OpVar` にする。
    per_operation: bool,
}
```

`fresh_rigid_with` は `per_operation: false` で作り、その直後に次を足す。

```rust
    /// 節のための具体化で、操作ごとに量化した型変数を作る。推論では `fresh_rigid_with` の変数と同じに扱う。
    pub fn fresh_operation_rigid(&mut self, name: &str, linearity: KindVar) -> Ty {
        self.rigids.push(RigidInfo {
            name: name.to_string(),
            linearity,
            per_operation: true,
        });
        let rigid = RigidVar(self.rigids.len() as u32 - 1);
        self.alloc(TyShape::Rigid(rigid))
    }
```

`shape.rs` の `instantiate_with_effect_args` の `None => table.fresh_rigid_with(name, lin[mu.index()]).0,` を `None => table.fresh_operation_rigid(name, lin[mu.index()]),` にする。

`table/export.rs` の `TyShape::Rigid` の腕を次にする。

```rust
            TyShape::Rigid(rigid) => {
                let info = &table.rigids[rigid.0 as usize];
                if info.per_operation {
                    TypeKind::OpVar(info.name.clone())
                } else {
                    TypeKind::Rigid(info.name.clone())
                }
            }
```

`crates/eml_core_ir/src/translate/types.rs` の `repr` の `TypeKind::Fn { .. } | TypeKind::Rigid(_) | TypeKind::Flexible => Repr::TObj,` に `TypeKind::OpVar(_)` を足す。`crates/eml_core_ir/tests/externs.rs` の `has_type_var` の `TypeKind::Rigid(_) | TypeKind::Flexible => true,` に `TypeKind::OpVar(_)` を足す (機械的な追随)。

`instantiations.rs` の `a_clause_variable_is_shown_like_the_function_variable_of_the_same_name` のコメントを「節の `x` の型は操作ごとの型変数 `a` で、関数の `a` と別の型 (`OpVar`) だが、同じ名前で表示する」にする (機械的な追随。スナップショットは変えない)。

- [ ] **Step 4: 通ることを確かめる**

Run: `cargo test -p eml_types --test integration instantiations::`
Expected: PASS (既存のスナップショットも変わらない)

- [ ] **Step 5: 代入と表示の失敗するテストを書く**

`store.rs` の `mod tests` の末尾に足す。

```rust
    fn rigid(types: &mut TypeStore, name: &str) -> TypeId {
        types.intern(TypeKind::Rigid(name.to_string()))
    }

    fn function(types: &mut TypeStore, param: TypeId, ret: TypeId) -> TypeId {
        types.intern(TypeKind::Fn {
            param,
            effects: vec![],
            tail: None,
            ret,
        })
    }

    #[test]
    fn substitution_replaces_signature_variables_inside_every_child() {
        let program = program();
        let state = effect(&program, "State");
        let mut types = TypeStore::new(&program);
        let (int, string) = (types.int(), types.string());
        let a = rigid(&mut types, "a");
        let b = rigid(&mut types, "b");
        let pair = types.intern(TypeKind::Record(vec![
            ("0".to_string(), a),
            ("1".to_string(), b),
        ]));
        let ty = types.intern(TypeKind::Fn {
            param: pair,
            effects: vec![EffectLabel {
                id: state,
                args: vec![a],
            }],
            tail: Some(RowTail::Rigid("e".to_string())),
            ret: a,
        });
        let mut subst = Substitution::new([("a".to_string(), int), ("b".to_string(), string)]);
        let substituted = types.substitute(ty, &mut subst);
        assert_eq!(
            types.display(substituted, &program.names).to_string(),
            "(Int, String) -> <State Int | e> Int"
        );
        // 同じ形の型は同じ ID になる
        let expected_pair = types.intern(TypeKind::Record(vec![
            ("0".to_string(), int),
            ("1".to_string(), string),
        ]));
        assert_eq!(types.substitute(pair, &mut subst), expected_pair);
    }

    #[test]
    fn operation_variables_become_flexible_and_unknown_names_stay() {
        let program = program();
        let mut types = TypeStore::new(&program);
        let int = types.int();
        let op = types.intern(TypeKind::OpVar("a".to_string()));
        let c = rigid(&mut types, "c");
        let ty = function(&mut types, op, c);
        let mut subst = Substitution::new([("a".to_string(), int)]);
        let substituted = types.substitute(ty, &mut subst);
        let expected = function(&mut types, types.flexible(), c);
        assert_eq!(substituted, expected);
    }

    #[test]
    fn types_without_variables_are_returned_as_they_are() {
        let program = program();
        let mut types = TypeStore::new(&program);
        let int = types.int();
        let ty = function(&mut types, int, int);
        let a = rigid(&mut types, "a");
        assert!(!types.contains_type_vars(ty));
        assert!(types.contains_type_vars(a));
        let mut subst = Substitution::new([("a".to_string(), int)]);
        assert_eq!(types.substitute(ty, &mut subst), ty);
    }

    #[test]
    fn a_shared_type_is_substituted_once_per_node() {
        // `t(i) = t(i-1) -> t(i-1)` は木として 2^i の大きさだが、表には i 個の節点しかない。指数の時間なら終わらない
        let program = program();
        let mut types = TypeStore::new(&program);
        let mut ty = rigid(&mut types, "a");
        for _ in 0..200 {
            ty = function(&mut types, ty, ty);
        }
        let int = types.int();
        let mut subst = Substitution::new([("a".to_string(), int)]);
        let substituted = types.substitute(ty, &mut subst);
        assert!(!types.contains_type_vars(substituted));
    }

    #[test]
    fn a_bounded_display_stops_at_the_limit() {
        let program = program();
        let mut types = TypeStore::new(&program);
        let int = types.int();
        let short = function(&mut types, int, int);
        assert_eq!(
            types.display_bounded(short, &program.names, 64).as_deref(),
            Some("Int -> Int")
        );
        assert_eq!(types.display_bounded(short, &program.names, 9), None);
        // 表示が 2^200 の長さになる型でも、上限で止まる
        let mut ty = int;
        for _ in 0..200 {
            ty = function(&mut types, ty, ty);
        }
        assert_eq!(types.display_bounded(ty, &program.names, 64), None);
    }
```

- [ ] **Step 6: 失敗を確かめる**

Run: `cargo test -p eml_types --lib store::`
Expected: コンパイルエラー (`Substitution`、`substitute`、`contains_type_vars`、`display_bounded` がない)

- [ ] **Step 7: 型変数を含むかの印と `Substitution` を入れる**

`TypeStore` に `errors` と並べて印を足す。

```rust
    /// 型が型変数 (`Rigid` か `OpVar`) を含むか。代入が、型変数を含まない型をたどらずに返すために使う。
    vars: Vec<bool>,
```

`TypeStore::new` の初期化に `vars: Vec::new(),` を足す。`intern` は、`errors` と同じく子から印を求める。

```rust
        let mut error = matches!(
            kind,
            TypeKind::Error
                | TypeKind::Fn {
                    tail: Some(RowTail::Error),
                    ..
                }
        );
        let mut var = matches!(kind, TypeKind::Rigid(_) | TypeKind::OpVar(_));
        kind.for_each_child(|child| {
            error |= self.errors[child.index()];
            var |= self.vars[child.index()];
        });
        let id = TypeId(self.kinds.len() as u32);
        self.kinds.push(kind.clone());
        self.errors.push(error);
        self.vars.push(var);
```

`intern` の doc コメントの「型を作るのは型検査だけである。」を「型を作るのは型検査と `substitute` だけである。」にする。`TypeStore` の doc コメントの「後の段階は表を読むだけで、型を作らない。」を「後の段階が型を足すのは、`substitute` での代入だけである。」にする。

`TypeStore` の `impl` に足す。

```rust
    pub fn contains_type_vars(&self, id: TypeId) -> bool {
        self.vars[id.index()]
    }

    /// `ty` の `Rigid` を `subst` の型に置き換え、`OpVar` を `Flexible` に置き換えた型。`subst` にない名前の `Rigid`
    /// は残す。row の末尾は変えない。単相化が instance の型を作るのに使う (docs/spec/core-ir.md の「変換の規則」)。
    pub fn substitute(&mut self, ty: TypeId, subst: &mut Substitution) -> TypeId {
        if !self.vars[ty.index()] {
            return ty;
        }
        if let Some(&done) = subst.done.get(&ty) {
            return done;
        }
        let kind = match self.kind(ty).clone() {
            TypeKind::Rigid(name) => {
                let replaced = subst.vars.get(&name).copied().unwrap_or(ty);
                subst.done.insert(ty, replaced);
                return replaced;
            }
            // 節は操作がどの型で呼ばれても動くので、関数の型引数と名前が同じでも置き換えず、一様にする
            TypeKind::OpVar(_) => {
                subst.done.insert(ty, self.flexible);
                return self.flexible;
            }
            TypeKind::Con { id, args } => TypeKind::Con {
                id,
                args: args.into_iter().map(|arg| self.substitute(arg, subst)).collect(),
            },
            TypeKind::Record(fields) => TypeKind::Record(
                fields
                    .into_iter()
                    .map(|(label, field)| (label, self.substitute(field, subst)))
                    .collect(),
            ),
            TypeKind::Fn {
                param,
                effects,
                tail,
                ret,
            } => TypeKind::Fn {
                param: self.substitute(param, subst),
                effects: effects
                    .into_iter()
                    .map(|label| EffectLabel {
                        id: label.id,
                        args: label
                            .args
                            .into_iter()
                            .map(|arg| self.substitute(arg, subst))
                            .collect(),
                    })
                    .collect(),
                tail,
                ret: self.substitute(ret, subst),
            },
            TypeKind::Flexible | TypeKind::Error => {
                unreachable!("a type without variables is returned before the match")
            }
        };
        let substituted = self.intern(kind);
        subst.done.insert(ty, substituted);
        substituted
    }
```

`store.rs` の `TypeStore` の定義の前に足す。

```rust
/// シグネチャの型変数への代入。代入した結果を型ごとに覚えるので、部分を共有する型を何度たどっても、1つの代入では
/// 各節点を1回だけ書き換える (docs/implementation/architecture.md の「`eml_types` の内部」)。
#[derive(Debug, Default)]
pub struct Substitution {
    vars: HashMap<String, TypeId>,
    done: HashMap<TypeId, TypeId>,
}

impl Substitution {
    pub fn new(vars: impl IntoIterator<Item = (String, TypeId)>) -> Substitution {
        Substitution {
            vars: vars.into_iter().collect(),
            done: HashMap::new(),
        }
    }
}
```

`lib.rs` の `store` の再公開 (`pub use store::{…}`) に `Substitution` を足す。

- [ ] **Step 8: `display_bounded` を入れ、表示が途中の文字列を作らないようにする**

今の `atomic` と `row_text` は、子の表示を `String` に作ってから書く。`display_bounded` を上限で止めるには、子も書き込み先へ直接書く必要がある。`atomic` と `row_text` を、`String` を返す関数から、`fmt::Display` を実装する小さな値に変える。

```rust
/// 型の適用の引数の位置に置く形。関数型と、引数を持つ型の適用は括弧で囲む。途中の文字列を作らずに書く。
struct Atomic<'a> {
    id: TypeId,
    types: &'a TypeStore,
    names: &'a DisplayNames,
}

impl fmt::Display for Atomic<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let shown = self.types.display(self.id, self.names);
        match self.types.kind(self.id) {
            TypeKind::Fn { .. } => write!(f, "({shown})"),
            TypeKind::Con { args, .. } if !args.is_empty() => write!(f, "({shown})"),
            _ => write!(f, "{shown}"),
        }
    }
}

fn atomic<'a>(types: &'a TypeStore, id: TypeId, names: &'a DisplayNames) -> Atomic<'a> {
    Atomic { id, types, names }
}
```

`row_text` は、`TypeDisplay` の `Fn` の腕の中で直接書く形にする。今の出力 (空の閉じた row は書かない、ラベルの後に `| 末尾`) を変えない。

```rust
            TypeKind::Fn {
                param,
                effects,
                tail,
                ret,
            } => {
                if matches!(types.kind(*param), TypeKind::Fn { .. }) {
                    write!(f, "({}) -> ", types.display(*param, names))?;
                } else {
                    write!(f, "{} -> ", types.display(*param, names))?;
                }
                // 空の閉じた row は書かない。省略した row が `<>` だから (docs/spec/types.md)
                let tail = match tail {
                    Some(RowTail::Rigid(name)) => Some(name.as_str()),
                    Some(RowTail::Flexible) => Some("_"),
                    Some(RowTail::Error) => Some("{error}"),
                    None => None,
                };
                if !effects.is_empty() || tail.is_some() {
                    f.write_str("<")?;
                    for (index, label) in effects.iter().enumerate() {
                        if index > 0 {
                            f.write_str(", ")?;
                        }
                        write!(f, "{}", label.display(types, names))?;
                    }
                    match tail {
                        Some(tail) if effects.is_empty() => f.write_str(tail)?,
                        Some(tail) => write!(f, " | {tail}")?,
                        None => {}
                    }
                    f.write_str("> ")?;
                }
                write!(f, "{}", types.display(*ret, names))
            }
```

`row_text` を使う箇所がほかにあれば (`grep -n row_text crates/eml_types/src`)、同じ形に直すか、`row_text` を残してそこからだけ使う。

`TypeStore` の `impl` に足す。

```rust
    /// 表示が `limit` 文字以下ならその文字列、超えたら `None`。表示は型を木としてたどるので、部分を共有する型では
    /// 長さが表の大きさの指数になる。上限を超えた時点で書くのをやめ、費用を `limit` に比例させる。
    pub fn display_bounded(&self, id: TypeId, names: &DisplayNames, limit: usize) -> Option<String> {
        let mut out = Bounded {
            text: String::new(),
            chars: 0,
            limit,
        };
        fmt::write(&mut out, format_args!("{}", self.display(id, names))).ok()?;
        Some(out.text)
    }
```

```rust
/// `limit` 文字を超えると書き込みを断る書き込み先。断ると、表示は `?` で途中から戻る。
struct Bounded {
    text: String,
    chars: usize,
    limit: usize,
}

impl fmt::Write for Bounded {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.chars += s.chars().count();
        if self.chars > self.limit {
            return Err(fmt::Error);
        }
        self.text.push_str(s);
        Ok(())
    }
}
```

`TypeDisplay` と `LabelDisplay` の中で子を書く箇所が、すべて `write!` か `f.write_str` で、`format!` や `to_string()` で途中の文字列を作っていないことを確かめる (`grep -n "format!\|to_string()" crates/eml_types/src/store.rs` の結果が、テストと `is_tuple` の比較だけになる)。

- [ ] **Step 9: 通ることを確かめる**

Run: `cargo test -p eml_types`
Expected: PASS (表示の既存のテストも変わらない)

- [ ] **Step 10: 全体を確かめる**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check && cargo test -p eml_cli --test integration citations`
Expected: すべて通る。UI テストと Core IR のスナップショットは変わらない (`git status --short` に `.snap` の変更がない)

- [ ] **Step 11: コミット**

```bash
git add crates/eml_types/src/store.rs crates/eml_types/src/lib.rs crates/eml_types/src/table/mod.rs crates/eml_types/src/table/export.rs crates/eml_types/src/shape.rs crates/eml_types/tests/instantiations.rs crates/eml_core_ir/src/translate/types.rs crates/eml_core_ir/tests/externs.rs
git commit -m "Add OpVar, Substitution and display_bounded to eml_types"
```

(メッセージの末尾に Global Constraints の2行を付ける。本文に、Task 1 では出力が変わらないことを書く)

---

### Task 2: 引用符で囲む関数の名前

**Files:**
- Modify: `crates/eml_core_ir/src/pretty.rs`
- Modify: `crates/eml_core_ir/src/text.rs`
- Modify: `crates/eml_core_ir/tests/common/mod.rs` (`function`)
- Test: `crates/eml_core_ir/tests/text.rs`

**Interfaces:**
- Produces: `pretty` は、関数の名前が空のとき、または空白、`(`、`)`、`{`、`}`、`[`、`]`、`,`、`"` のどれかを含むとき、名前を Rust の `{:?}` で書く。`parse` は `fn`、`call`、`closure`、`&` の名前の位置で文字列の字句を受け付ける (`tail` は `call` を経由する)。テストの `function(shown, name)` は、引用符で囲んだ名前の関数も見つける

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_core_ir/tests/text.rs` の `a_closure_round_trips` の後に足す。

```rust
#[test]
fn quoted_function_names_round_trip() {
    // 単相化の instance の名前は空白と括弧を含むので、引用符で囲んで書く (docs/implementation/testing.md の
    // 「Core IR のテキストの形」)
    let program = round_trip(
        "\
fn \"id@[Int, String]\"(x.0: int) -> int {
  let c.1: tobj = closure \"id@[Int, String]\"(x.0)
  let r.2: tobj = apply c.1(&\"id@[Int, String]\")
  let y.3: int = call \"id@[Int, String]\"(x.0)
  tail call \"id@[Int, String]\"(y.3)
}
",
    );
    assert_eq!(program.functions[0].name, "id@[Int, String]");
    assert_eq!(
        program.functions[0].blocks[0].stmts[0],
        Stmt::Let {
            var: VarId(1),
            rhs: Rhs::MakeClosure(FnIdx(0), vec![v(0)]),
        }
    );
}
```

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_core_ir --test integration text::quoted_function_names_round_trip`
Expected: FAIL (`expected a name, found "id@[Int, String]"` などの読みの誤り)

- [ ] **Step 3: `pretty` で引用符で囲む**

`pretty.rs` に足す。

```rust
/// 関数の名前。単相化の instance の名前 (`id@[Int, String]`) は、テキストの形の字句の区切りを含むので、文字列と同じ
/// 逃がし方で引用符で囲む (docs/implementation/testing.md の「Core IR のテキストの形」)。
fn function_name(name: &str) -> std::borrow::Cow<'_, str> {
    let separates = |c: char| c.is_whitespace() || "(){}[],\"".contains(c);
    if name.is_empty() || name.contains(separates) {
        format!("{name:?}").into()
    } else {
        name.into()
    }
}
```

関数の名前を書く4か所 (`fn` の見出し、`closure`、`call`、`&`) を、`function.name` と `self.program.function(*target).name` の代わりに `function_name(&…)` で書く。`&` の箇所は `format!("&{}", function_name(&self.program.function(*target).name))` にする。

- [ ] **Step 4: `parse` で文字列の名前を受け付ける**

`text.rs` に足す。

```rust
    /// 関数の名前。引用符で囲んだ名前 (`pretty` の `function_name`) も受け付ける。
    fn name(&mut self) -> Result<String, ParseError> {
        if let Some(Tok::Str(value)) = self.peek() {
            let value = value.clone();
            self.pos += 1;
            return Ok(value);
        }
        self.word()
    }
```

次の箇所を直す。

- `declare_functions`: `fn` の次の字句が `Tok::Word(name)` のときだけでなく `Tok::Str(name)` のときも名前として登録する (`match` の腕を2つにするか、`Tok::Word(name) | Tok::Str(name)` でまとめる)
- `function`: `let name = self.word()?;` を `let name = self.name()?;` にする
- `function_name`: `let name = self.word()?;` を `let name = self.name()?;` にする
- `atom`: `let word = self.word()?;` の後、`if let Some(name) = word.strip_prefix('&')` の前に、`&` の直後に文字列が続く形を読む

```rust
        if word == "&"
            && let Some(Tok::Str(name)) = self.peek()
        {
            let name = name.clone();
            self.pos += 1;
            return Ok(Atom::Fn(self.resolve_function(&name, line)?));
        }
```

- [ ] **Step 5: テストの補助関数を直す**

`crates/eml_core_ir/tests/common/mod.rs` の `function` が、引用符で囲んだ見出しも探すようにする (機械的な追随)。

```rust
pub fn function(shown: &str, name: &str) -> String {
    let header = shown
        .find(&format!("fn {name}("))
        .or_else(|| shown.find(&format!("fn {name:?}(")))
        .unwrap_or_else(|| panic!("no `{name}` in\n{shown}"));
```

(以降は今のまま)

- [ ] **Step 6: 通ることを確かめる**

Run: `cargo test -p eml_core_ir --test integration text::`
Expected: PASS

- [ ] **Step 7: 全体を確かめる**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る。スナップショットは変わらない (今の名前はどれも引用符を要しない)

- [ ] **Step 8: コミット**

```bash
git add crates/eml_core_ir/src/pretty.rs crates/eml_core_ir/src/text.rs crates/eml_core_ir/tests/common/mod.rs crates/eml_core_ir/tests/text.rs
git commit -m "Quote Core IR function names that the text lexer would split"
```

(メッセージの末尾に Global Constraints の2行を付ける)

---

### Task 3: instance の表への置き換え (鍵は型引数なし)

**Files:**
- Create: `crates/eml_core_ir/src/translate/instances.rs`
- Modify: `crates/eml_core_ir/src/translate/mod.rs` (`reachable` の削除、`translate`、`BodyCtx`)
- Modify: `crates/eml_core_ir/src/translate/expr.rs` (`Callee::Function` と関数値の参照)

**Interfaces:**
- Consumes: Task 1 の `TypeStore` (この Task では代入を使わない)
- Produces (Task 4 が広げる)
  - `pub(super) struct InstanceId(pub(super) usize)`
  - `pub(super) struct Instance { function: FunctionId, args: Vec<TypeId>, name: String, signature: TypeId, types: Option<BodyTypes>, targets: ArenaMap<ExprId, InstanceId> }` (フィールドはすべて `pub(super)`)
  - `pub(super) struct Instances { list: Vec<Instance>, entry: InstanceId, store: TypeStore }`
  - `pub(super) fn collect(hir: &HirProgram, typed: &TypedProgram, entry: FunctionId) -> Instances`
  - `BodyCtx` の `indices: &ItemMap<Function, FnIdx>` を、`targets: &ArenaMap<ExprId, InstanceId>` と `indices: &[FnIdx]` に替え、`BodyCtx::target(&self, expr: ExprId) -> FnIdx` を足す

この Task は振る舞いを変えないリファクタリングである。テストは今のテスト全体で、出力が1バイトも変わらないことを確かめる。

- [ ] **Step 1: 今のテストが通っている基準を記録する**

Run: `cargo test 2>&1 | grep "test result" | awk '{p+=$4; f+=$6} END {print p" passed, "f" failed"}'`
Expected: すべて通る (件数を控える)

- [ ] **Step 2: `instances.rs` を作る**

```rust
//! 単相化の instance の表 (docs/spec/core-ir.md の「変換の規則」)。本体を変換する前に、入口から届く (関数, 型引数)
//! の組をすべて集め、番号の順を決める。変換はこの表を読むだけで、変換の途中で instance を足さない。

use std::collections::{HashMap, VecDeque};

use eml_hir::{ExprId, FunctionId, FunctionKind, Program as HirProgram, ValueItem};
use eml_types::{BodyTypes, TypeId, TypeStore, TypedProgram};
use la_arena::ArenaMap;

use super::program::core_name;

/// `Instances::list` の位置。変換はこの順に関数の番号を予約する。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(super) struct InstanceId(pub(super) usize);

pub(super) struct Instance {
    pub(super) function: FunctionId,
    /// シグネチャの型変数の順の型引数。
    pub(super) args: Vec<TypeId>,
    pub(super) name: String,
    /// 宣言の型に instance の代入をかけた型。引数と `ret` の Repr を決める。
    pub(super) signature: TypeId,
    /// 本体の型に代入をかけた表。`None` なら型検査の表をそのまま使う。
    pub(super) types: Option<BodyTypes>,
    /// 本体の中の、定義された関数への参照の行き先。extern の関数への参照は入らない。
    pub(super) targets: ArenaMap<ExprId, InstanceId>,
}

pub(super) struct Instances {
    pub(super) list: Vec<Instance>,
    pub(super) entry: InstanceId,
    /// 型検査の型の表に、代入の結果を足した表。
    pub(super) store: TypeStore,
}

/// 見つけた順の instance。番号の順は、すべて見つけてから決める。
struct Found {
    function: FunctionId,
    args: Vec<TypeId>,
    signature: TypeId,
    types: Option<BodyTypes>,
    targets: ArenaMap<ExprId, usize>,
}

pub(super) fn collect(hir: &HirProgram, typed: &TypedProgram, entry: FunctionId) -> Instances {
    let store = typed.types.clone();
    let mut found: Vec<Found> = Vec::new();
    let mut keys: HashMap<(FunctionId, Vec<TypeId>), usize> = HashMap::new();
    let mut queue = VecDeque::new();
    let mut add = |function: FunctionId,
                   args: Vec<TypeId>,
                   found: &mut Vec<Found>,
                   queue: &mut VecDeque<usize>| {
        *keys.entry((function, args.clone())).or_insert_with(|| {
            found.push(Found {
                function,
                args,
                signature: typed.decls[&ValueItem::Function(function)].ty,
                types: None,
                targets: ArenaMap::default(),
            });
            queue.push_back(found.len() - 1);
            found.len() - 1
        })
    };
    let entry_index = add(entry, Vec::new(), &mut found, &mut queue);
    // 先に見つけた instance から順に本体をたどる。番号の順と名前が変換の順によらないようにするため
    while let Some(index) = queue.pop_front() {
        let function = found[index].function;
        let body = typed
            .bodies
            .get(function)
            .expect("every reached body is type-checked");
        let mut targets = ArenaMap::default();
        for (expr, instantiation) in body.instantiations.iter() {
            let ValueItem::Function(callee) = instantiation.decl else {
                continue;
            };
            if hir[callee].kind != FunctionKind::Defined {
                continue;
            }
            let target = add(callee, Vec::new(), &mut found, &mut queue);
            targets.insert(expr, target);
        }
        found[index].targets = targets;
    }
    order(hir, found, entry_index, store)
}

/// HIR の関数の順に並べ、同じ関数の instance は見つけた順に並べる。`TypeId` の値の順には並べない。後の段階が
/// `TypeId` の値に意味を持たせないためである。
fn order(hir: &HirProgram, found: Vec<Found>, entry: usize, store: TypeStore) -> Instances {
    let position: HashMap<FunctionId, usize> = hir
        .functions()
        .enumerate()
        .map(|(position, (id, _))| (id, position))
        .collect();
    let mut sorted: Vec<usize> = (0..found.len()).collect();
    sorted.sort_by_key(|&index| (position[&found[index].function], index));
    let mut renumbered = vec![0; found.len()];
    for (new, &old) in sorted.iter().enumerate() {
        renumbered[old] = new;
    }
    let mut found: Vec<Option<Found>> = found.into_iter().map(Some).collect();
    let list = sorted
        .iter()
        .map(|&old| {
            let found = found[old].take().expect("each instance is taken once");
            Instance {
                name: core_name(hir, found.function.module, &hir[found.function].name),
                function: found.function,
                args: found.args,
                signature: found.signature,
                types: found.types,
                targets: found
                    .targets
                    .iter()
                    .map(|(expr, &target)| (expr, InstanceId(renumbered[target])))
                    .collect(),
            }
        })
        .collect();
    Instances {
        list,
        entry: InstanceId(renumbered[entry]),
        store,
    }
}
```

`translate/mod.rs` の `mod` の並びに `mod instances;` を足す。`ArenaMap` を `collect` で作れない場合は、`let mut map = ArenaMap::default(); for … { map.insert(…) }` で作る。

- [ ] **Step 3: `translate` を instance の表で回す**

`translate/mod.rs` の `reachable` を削除する。`translate` の、`let mut indices = ItemMap::default();` から `builder.finish(indices[id], core);` までと、入口の関数を作る箇所を次にする。

```rust
    let instances = instances::collect(hir, typed, entry);
    let store = &instances.store;
    let indices: Vec<FnIdx> = instances
        .list
        .iter()
        .map(|instance| {
            let body = hir
                .body(instance.function)
                .expect("a program without errors has an equation for every function");
            builder.reserve(body.params.len())
        })
        .collect();
    for (instance, &index) in instances.list.iter().zip(&indices) {
        let id = instance.function;
        let body = hir.body(id).expect("checked above");
        let (param_types, ret) = split_arrows(store, instance.signature, body.params.len());
        let params: Vec<(Option<PatId>, Repr)> = body
            .params
            .iter()
            .zip(&param_types)
            .map(|(&pat, &ty)| (Some(pat), repr(store, ty, hir)))
            .collect();
        let forms = continuation_forms(body);
        let numbers = numbering(hir, body);
        let ctx = BodyCtx {
            hir,
            body,
            store,
            types: instance
                .types
                .as_ref()
                .unwrap_or_else(|| typed.bodies.get(id).expect("every body is type-checked")),
            targets: &instance.targets,
            indices: &indices,
            root_name: &instance.name,
            numbering: &numbers,
            continuation_forms: &forms,
            source: Source {
                files,
                file_id: hir.modules[id.module].file,
            },
        };
        let core = FnLowering::new(ctx, &mut builder).lower(
            &instance.name,
            false,
            &[],
            &params,
            body.root,
            repr(store, ret, hir),
        );
        builder.finish(index, core);
    }
    let entry_instance = &instances.list[instances.entry.0];
    let entry_fn = builder.entry(
        hir,
        store,
        indices[instances.entry.0],
        entry,
        entry_instance.signature,
    );
```

`BodyCtx` の `indices: &'a ItemMap<Function, FnIdx>,` を次の2つにし、`store` のコメントを直す。

```rust
    /// 型の表。型検査の表に、単相化の代入の結果を足したもの。本体の変換は読むだけである。
    store: &'a TypeStore,
    types: &'a BodyTypes,
    /// 本体の中の、定義された関数への参照の行き先の instance。
    targets: &'a ArenaMap<ExprId, InstanceId>,
    /// instance の番号から関数の番号への表。
    indices: &'a [FnIdx],
```

`BodyCtx` の `impl` を足す。

```rust
impl BodyCtx<'_> {
    /// 定義された関数への参照 `expr` の行き先の関数。
    fn target(&self, expr: ExprId) -> FnIdx {
        let instance = self
            .targets
            .get(expr)
            .expect("every reference to a defined function has an instance");
        self.indices[instance.0]
    }
}
```

`translate/expr.rs` の `None => Callee::Function(self.ctx.indices[*function]),` を `None => Callee::Function(self.ctx.target(callee)),` に、`let target = self.ctx.indices[*function];` を `let target = self.ctx.target(id);` にする。使われなくなった `use` (`HashSet`、`Function`、`ItemMap` など) は `cargo clippy` の指摘に従って消す。`instances::InstanceId` を `use` に足す。

- [ ] **Step 4: 出力が変わらないことを確かめる**

Run: `cargo test 2>&1 | grep "test result" | awk '{p+=$4; f+=$6} END {print p" passed, "f" failed"}' && git status --short`
Expected: Step 1 と同じ件数がすべて通る。`.snap` のファイルと、テストのファイルに変更がない

- [ ] **Step 5: 全体を確かめる**

Run: `cargo clippy --all-targets && cargo fmt --check && cargo test -p eml_cli --test integration citations`
Expected: 警告なし、差分なし、通る

- [ ] **Step 6: コミット**

```bash
git add crates/eml_core_ir/src/translate/instances.rs crates/eml_core_ir/src/translate/mod.rs crates/eml_core_ir/src/translate/expr.rs
git commit -m "Translate through an instance table instead of a reachable set"
```

(メッセージの末尾に Global Constraints の2行を付ける。本文に、鍵はまだ型引数を持たず、出力が変わらないことを書く)

---

### Task 4: 型引数の鍵、多相再帰、代入、名前

**Files:**
- Modify: `crates/eml_core_ir/src/translate/instances.rs`
- Create: `crates/eml_core_ir/tests/instances.rs`
- Modify: `crates/eml_core_ir/tests/main.rs` (`mod instances;`)
- Modify: `crates/eml_core_ir/tests/translate.rs`、`crates/eml_core_ir/tests/boxing.rs`、`crates/eml_core_ir/tests/perceus.rs` (期待値と、boxing.rs の2件の置き換え)
- Modify: `crates/eml_interp/tests/bench.rs` (回数のスナップショット)
- Create: `tests/ui/run/functions/polymorphic_recursion.em` と、その UI のスナップショット

**Interfaces:**
- Consumes: Task 1 の `Substitution`、`substitute`、`contains_type_vars`、`display_bounded`。Task 3 の `Instances`
- Produces: instance の名前 `元の名前@[型引数, …]`、`元の名前@N`。`Instance.args` は代入と一様な位置を反映した鍵、`Instance.types` は型変数を持つ関数で `Some`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_core_ir/tests/instances.rs` を作り、`tests/main.rs` の `mod` の並びに `mod instances;` を足す。

```rust
//! 単相化の instance (docs/spec/core-ir.md の「変換の規則」)。どの (関数, 型引数) が instance になり、どう名付けるかを
//! 確かめる。

use crate::common::{core_text, function};
use eml_core_ir::Pass;

/// `Pass::Translate` までの Core IR の、内部の印のない関数のうち、`name@` で始まる名前を並べる。
fn instances_of(text: &str, name: &str) -> Vec<String> {
    let program = eml_test_support::core_until(text, Pass::Translate);
    let prefix = format!("{name}@");
    program
        .functions
        .iter()
        .filter(|function| !function.internal && function.name.starts_with(&prefix))
        .map(|function| function.name.clone())
        .collect()
}

#[test]
fn a_chain_used_at_two_types_has_two_instances_per_function() {
    let n = 20;
    let mut text = String::new();
    for i in 1..n {
        text.push_str(&format!("f{i} : a -> a\nf{i} x = f{} x\n\n", i + 1));
    }
    text.push_str(&format!("f{n} : a -> a\nf{n} x = x\n\n"));
    text.push_str("main : Unit -> <IO> Unit\nmain () = println (show_int (f1 1) ++ f1 \"s\")");
    for i in 1..=n {
        assert_eq!(
            instances_of(&text, &format!("f{i}")),
            [format!("f{i}@[Int]"), format!("f{i}@[String]")]
        );
    }
}

#[test]
fn one_function_used_at_k_types_has_k_instances() {
    let text = "id : a -> a\nid x = x\n\nmain : Unit -> <IO> Unit\nmain () =\n  let _ = id 1\n  let _ = id \"s\"\n  let _ = id True\n  let _ = id ()\n  let _ = id (1, \"s\")\n  ()";
    assert_eq!(
        instances_of(text, "id"),
        [
            "id@[Int]",
            "id@[String]",
            "id@[Bool]",
            "id@[Unit]",
            "id@[(Int, String)]"
        ]
    );
}

#[test]
fn polymorphic_recursion_is_uniform() {
    let text = "depth : Int -> a -> Int\ndepth n x = if n == 0 then 0 else 1 + depth (n - 1) (x, x)\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (depth 3 1))";
    assert_eq!(instances_of(text, "depth"), ["depth@[_]"]);
}

#[test]
fn mutual_polymorphic_recursion_is_uniform() {
    let text = "f : Int -> a -> Int\nf n x = if n == 0 then 0 else g (n - 1) (x, x)\n\ng : Int -> b -> Int\ng n y = f n y\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (f 3 1))";
    assert_eq!(instances_of(text, "f"), ["f@[_]"]);
    assert_eq!(instances_of(text, "g"), ["g@[_]"]);
}

#[test]
fn recursion_through_a_function_value_is_found() {
    // 自分を値として渡して呼ぶ形も、値の参照が具体化の表に入るので、大きくなる辺になる
    let text = "call : (Int -> b -> Int) -> Int -> b -> Int\ncall f n x = f n x\n\ndepth : Int -> a -> Int\ndepth n x = if n == 0 then 0 else call depth (n - 1) (x, x)\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (depth 3 1))";
    assert_eq!(instances_of(text, "depth"), ["depth@[_]"]);
}

#[test]
fn only_the_growing_position_is_uniform() {
    let text = "walk : Int -> a -> b -> b\nwalk n x y = if n == 0 then y else walk (n - 1) (x, x) y\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (walk 3 1 5))";
    assert_eq!(instances_of(text, "walk"), ["walk@[_, Int]"]);
    let shown = core_text(text, Pass::Translate);
    let header = function(&shown, "walk@[_, Int]");
    assert!(
        header.starts_with("fn \"walk@[_, Int]\"(n.0: int, x.1: tobj, y.2: int) -> int {"),
        "{header}"
    );
}

#[test]
fn a_generic_call_in_a_clause_at_the_clause_variable_is_uniform() {
    let text = "id : a -> a\nid x = x\n\neffect Pick where\n  pick : a -> a\n\nrun : Int -> Int\nrun v =\n  handle pick v with\n    | pick x k -> k (id x)\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (run 1))";
    assert_eq!(instances_of(text, "id"), ["id@[_]"]);
}

#[test]
fn a_clause_variable_that_escapes_through_the_handle_is_uniform() {
    let text = "\
effect Leak where
  put : a -> Unit
  never stop : Unit -> a

effect Abort where
  never abort : Unit -> a

id : a -> a
id x = x

pair : a -> (a, a)
pair x = (x, x)

body : Unit -> <Leak> b
body () =
  put 1
  stop ()

run : Unit -> <Abort> Unit
run () =
  let r = handle body () with
            | put x k ->
                drop k
                id x
            | stop u -> abort ()
  let p = pair r
  ()

main : Unit -> <IO> Unit
main () =
  handle run () with
    | abort u -> println \"aborted\"
  println \"done\"";
    assert_eq!(instances_of(text, "id"), ["id@[_]"]);
    assert_eq!(instances_of(text, "pair"), ["pair@[_]"]);
}

#[test]
fn a_long_shared_type_argument_gets_an_ordinal_name() {
    // `ident` の型引数は列の長さの指数の大きさで表示されるので、名前は順番の形になる
    let n = 64;
    let mut text = String::from("ident : a -> a\nident x = x\n\nrun : Unit -> Int\nrun () =\n  let f0 = ident\n");
    for i in 1..=n {
        text.push_str(&format!("  let f{i} = f{} ident\n", i - 1));
    }
    text.push_str(&format!("  f{n} 5\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (run ()))"));
    let names = instances_of(&text, "ident");
    assert_eq!(names.len(), n + 1);
    assert!(names.iter().any(|name| name.starts_with("ident@") && !name.starts_with("ident@[")), "{names:?}");
}

#[test]
fn monomorphic_programs_keep_their_function_order_and_names() {
    let text = "double : Int -> Int\ndouble n = n + n\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (double 2))";
    let program = eml_test_support::core_until(text, Pass::Translate);
    let names: Vec<&str> = program.functions.iter().map(|function| function.name.as_str()).collect();
    assert_eq!(names, ["double", "main", "entry$main"]);
}

#[test]
fn a_polymorphic_entry_is_uniform() {
    let checked = eml_test_support::check("first : a -> a\nfirst x = x\n\nmain : Unit -> <IO> Unit\nmain () = ()");
    let (entry, _) = checked
        .program
        .functions()
        .find(|(_, function)| function.name == "first")
        .unwrap();
    let program = eml_core_ir::lower_until(&checked.program, &checked.typed, entry, checked.files(), Pass::Translate);
    let names: Vec<&str> = program.functions.iter().map(|function| function.name.as_str()).collect();
    assert_eq!(names, ["first@[_]", "entry$first"]);
}
```

`monomorphic_programs_keep_their_function_order_and_names` の期待値の並びは、今の main の出力で確かめてから書く (`main` 以外の Prelude の関数が入るなら、それも並べる)。`lower_until` の名前と引数が違えば、`crates/eml_core_ir/src/pipeline.rs` の公開の関数に合わせる。`a_polymorphic_entry_is_uniform` の入口の関数は `Unit -> …` でなければ `entry` が作れないなら、`first : Unit -> a` などの形に直し、鍵がすべて `Flexible` になることだけを確かめる。

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_core_ir --test integration instances::`
Expected: FAIL (名前に `@` が付かない。`monomorphic_programs_keep_their_function_order_and_names` は通る)

- [ ] **Step 3: 一様な位置を求める**

`instances.rs` に足す。

```rust
/// 多相再帰で大きくなる型変数の位置 (docs/spec/core-ir.md の「変換の規則」)。節点は (関数, 型変数の番号) で、関数 f
/// の本体の参照 `g @[T0, …]` の Tj に f の型変数 i が現れるとき (f, i) から (g, j) へ辺を引く。Tj が i そのもの
/// でなければ大きくなる辺である。大きくなる辺の両端を含む強連結成分の節点を、一様な位置とする。
fn uniform_positions(hir: &HirProgram, typed: &TypedProgram) -> HashSet<(FunctionId, usize)> {
    let mut nodes: Vec<(FunctionId, usize)> = Vec::new();
    let mut node_of: HashMap<(FunctionId, usize), usize> = HashMap::new();
    for (id, function) in hir.functions() {
        if function.kind != FunctionKind::Defined {
            continue;
        }
        for position in 0..type_vars(hir, id).len() {
            node_of.insert((id, position), nodes.len());
            nodes.push((id, position));
        }
    }
    let mut edges: Vec<Vec<(usize, bool)>> = vec![Vec::new(); nodes.len()];
    for (f, function) in hir.functions() {
        if function.kind != FunctionKind::Defined {
            continue;
        }
        let names = type_vars(hir, f);
        if names.is_empty() {
            continue;
        }
        let Some(body) = typed.bodies.get(f) else {
            continue;
        };
        // 本体の中で共有された型を、各節点1回だけたどる
        let mut occurrences = HashMap::new();
        for (_, instantiation) in body.instantiations.iter() {
            let ValueItem::Function(g) = instantiation.decl else {
                continue;
            };
            if hir[g].kind != FunctionKind::Defined {
                continue;
            }
            for (j, &arg) in instantiation.args.iter().enumerate() {
                let occurs = vars_in(&typed.types, arg, &names, &mut occurrences);
                for (i, name) in names.iter().enumerate() {
                    if occurs[i] {
                        let grow = !matches!(typed.types.kind(arg), TypeKind::Rigid(n) if n == name);
                        edges[node_of[&(f, i)]].push((node_of[&(g, j)], grow));
                    }
                }
            }
        }
    }
    let component = strongly_connected(&edges);
    let mut growing = HashSet::new();
    for (from, out) in edges.iter().enumerate() {
        for &(to, grow) in out {
            if grow && component[from] == component[to] {
                growing.insert(component[from]);
            }
        }
    }
    nodes
        .into_iter()
        .enumerate()
        .filter(|(node, _)| growing.contains(&component[*node]))
        .map(|(_, position)| position)
        .collect()
}

/// シグネチャの型変数の名前。並びは具体化の表の型引数の順と同じである。
fn type_vars(hir: &HirProgram, function: FunctionId) -> Vec<String> {
    hir[function]
        .signature
        .as_ref()
        .map(|signature| {
            signature
                .generics
                .type_vars
                .iter()
                .map(|(_, var)| var.name.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// `ty` に現れる `names` の型変数 (番号ごとの真偽)。`substitute` がたどる位置 (型構成子の引数、関数型の引数と結果と
/// エフェクトの型引数、タプルの要素) を見る。結果を `memo` に覚え、共有された型は1回だけたどる。
fn vars_in(
    types: &TypeStore,
    ty: TypeId,
    names: &[String],
    memo: &mut HashMap<TypeId, Vec<bool>>,
) -> Vec<bool> {
    if !types.contains_type_vars(ty) {
        return vec![false; names.len()];
    }
    if let Some(found) = memo.get(&ty) {
        return found.clone();
    }
    let mut found = vec![false; names.len()];
    let mut merge = |child: Vec<bool>| {
        for (slot, child) in found.iter_mut().zip(child) {
            *slot |= child;
        }
    };
    match types.kind(ty) {
        TypeKind::Rigid(name) => {
            if let Some(position) = names.iter().position(|n| n == name) {
                merge({
                    let mut one = vec![false; names.len()];
                    one[position] = true;
                    one
                });
            }
        }
        TypeKind::Con { args, .. } => {
            for &arg in args {
                merge(vars_in(types, arg, names, memo));
            }
        }
        TypeKind::Record(fields) => {
            for &(_, field) in fields {
                merge(vars_in(types, field, names, memo));
            }
        }
        TypeKind::Fn {
            param, effects, ret, ..
        } => {
            merge(vars_in(types, *param, names, memo));
            for label in effects {
                for &arg in &label.args {
                    merge(vars_in(types, arg, names, memo));
                }
            }
            merge(vars_in(types, *ret, names, memo));
        }
        TypeKind::OpVar(_) | TypeKind::Flexible | TypeKind::Error => {}
    }
    memo.insert(ty, found.clone());
    found
}

/// 強連結成分の番号。Tarjan の方法を作業の列で行い、グラフの深さに比例して Rust のスタックを使わない。
fn strongly_connected(edges: &[Vec<(usize, bool)>]) -> Vec<usize> {
    const UNSEEN: usize = usize::MAX;
    let mut index = vec![UNSEEN; edges.len()];
    let mut low = vec![0; edges.len()];
    let mut on_stack = vec![false; edges.len()];
    let mut stack = Vec::new();
    let mut component = vec![UNSEEN; edges.len()];
    let (mut next_index, mut next_component) = (0, 0);
    for root in 0..edges.len() {
        if index[root] != UNSEEN {
            continue;
        }
        // (節点, 次に見る辺の位置)
        let mut work = vec![(root, 0)];
        index[root] = next_index;
        low[root] = next_index;
        next_index += 1;
        stack.push(root);
        on_stack[root] = true;
        while let Some(&(node, edge)) = work.last() {
            if let Some(&(to, _)) = edges[node].get(edge) {
                work.last_mut().expect("read above").1 += 1;
                if index[to] == UNSEEN {
                    index[to] = next_index;
                    low[to] = next_index;
                    next_index += 1;
                    stack.push(to);
                    on_stack[to] = true;
                    work.push((to, 0));
                } else if on_stack[to] {
                    low[node] = low[node].min(index[to]);
                }
                continue;
            }
            work.pop();
            if let Some(&(parent, _)) = work.last() {
                low[parent] = low[parent].min(low[node]);
            }
            if low[node] == index[node] {
                loop {
                    let member = stack.pop().expect("the root of a component is on the stack");
                    on_stack[member] = false;
                    component[member] = next_component;
                    if member == node {
                        break;
                    }
                }
                next_component += 1;
            }
        }
    }
    component
}
```

`use` に `std::collections::HashSet` と `eml_types::{Substitution, TypeKind}` を足す。

- [ ] **Step 4: 鍵に型引数を入れ、本体の型に代入をかける**

`collect` を次のように変える。

- 始めに `let uniform = uniform_positions(hir, typed);` とし、`store` を `let mut store = typed.types.clone();` にする
- 入口の鍵は、`type_vars(hir, entry)` の数だけ `store.flexible()` を並べた列にする
- `add` の中の `signature` は、ここでは宣言の型のまま入れ、下で代入をかけた型に置き換える
- 本体をたどる各 instance で、次を行う

```rust
        let names = type_vars(hir, function);
        let mut subst = Substitution::new(names.iter().cloned().zip(found[index].args.iter().copied()));
        let decl = typed.decls[&ValueItem::Function(function)].ty;
        found[index].signature = store.substitute(decl, &mut subst);
        let mut instantiations = ArenaMap::default();
        for (expr, instantiation) in body.instantiations.iter() {
            // 節の型変数 (`OpVar`) は、型変数を持たない関数でも `Flexible` にそろえる
            let args: Vec<TypeId> = instantiation
                .args
                .iter()
                .map(|&arg| store.substitute(arg, &mut subst))
                .collect();
            if let ValueItem::Function(callee) = instantiation.decl
                && hir[callee].kind == FunctionKind::Defined
            {
                let key: Vec<TypeId> = args
                    .iter()
                    .enumerate()
                    .map(|(position, &arg)| {
                        if uniform.contains(&(callee, position)) {
                            store.flexible()
                        } else {
                            arg
                        }
                    })
                    .collect();
                let target = add(callee, key, &mut found, &mut queue);
                targets.insert(expr, target);
            }
            instantiations.insert(
                expr,
                Instantiation {
                    decl: instantiation.decl,
                    args,
                },
            );
        }
        if !names.is_empty() {
            let mut each = |ty: &TypeId| store.substitute(*ty, &mut subst);
            let types = BodyTypes {
                exprs: body.exprs.iter().map(|(expr, ty)| (expr, each(ty))).collect(),
                locals: body.locals.iter().map(|(local, ty)| (local, each(ty))).collect(),
                pats: body.pats.iter().map(|(pat, ty)| (pat, each(ty))).collect(),
                instantiations,
                masks: body.masks.clone(),
            };
            debug_assert!(
                types.exprs.iter().all(|(_, &ty)| !store.contains_type_vars(ty)),
                "an instance body keeps no type variable"
            );
            found[index].types = Some(types);
        }
```

`add` はクロージャのまま `store` を借りないようにする (`store.flexible()` は `add` の外で求める)。借用が合わなければ、`add` を `fn` にして `keys` と `found` と `queue` を引数で渡す。`ArenaMap` を `collect` で作れなければ `insert` の繰り返しにする。`Instantiation` を `use` に足す。`BodyTypes` に `#[non_exhaustive]` や公開されていないフィールドがあれば、`eml_types` に `BodyTypes` を組み立てる公開の関数を足す (Task 1 の範囲のファイルを直すことになるので、台帳に判断として書く)。

型変数を持たない関数 (`names` が空) の本体は、`exprs`、`locals`、`pats` に型変数を含まない (節の `OpVar` は `repr` が `tobj` にする) ので写さない。その場合も、`instantiations` の型引数の代入 (`OpVar` を `Flexible` にする) は鍵を作るのに使う。

- [ ] **Step 5: 名前を付ける**

`order` の名前の作り方を変える。

```rust
/// `@[` と `]` の間の上限の文字数。部分を共有する型では表示が指数の長さになるので、超えたら順番の名前にする。
const NAME_LIMIT: usize = 64;

/// 型変数を持つ関数の instance の名前 (docs/spec/core-ir.md の「変換の規則」)。`ordinal` は、その関数の instance の
/// 中の順番 (1 から数える) である。
fn instance_name(
    store: &TypeStore,
    hir: &HirProgram,
    base: &str,
    args: &[TypeId],
    ordinal: usize,
    taken: &mut HashSet<String>,
) -> String {
    let shown: Option<Vec<String>> = args
        .iter()
        .map(|&arg| store.display_bounded(arg, &hir.names, NAME_LIMIT))
        .collect();
    let inner = shown
        .map(|shown| shown.join(", "))
        .filter(|inner| inner.chars().count() <= NAME_LIMIT);
    if let Some(inner) = inner {
        let name = format!("{base}@[{inner}]");
        // 異なる型が同じ表示になる (row の末尾の表示など) ときは、後の instance を順番の名前にする
        if taken.insert(name.clone()) {
            return name;
        }
    }
    let name = format!("{base}@{ordinal}");
    taken.insert(name.clone());
    name
}
```

`order` では、並べた後の順に instance を見て、関数ごとに順番を数える (`HashMap<FunctionId, usize>`)。`type_vars(hir, function)` が空なら今と同じ `core_name(…)`、空でなければ `instance_name(&store, hir, &core_name(…), &args, ordinal, &mut taken)` を名前にする。`taken` は関数ごとでなく1つでよい (根の名前が違えば重ならない)。

- [ ] **Step 6: instance のテストが通ることを確かめる**

Run: `cargo test -p eml_core_ir --test integration instances::`
Expected: PASS

- [ ] **Step 7: 期待値の変わったテストを読み、受け入れる**

Run: `cargo insta test -p eml_core_ir --review` (対話ができない環境では `cargo insta test -p eml_core_ir` の後、`cargo insta pending-list` と各 `.snap.new` / インラインの差分を読む)

spec の「テストの変更」に挙げた次のテストの差分を1つずつ読む: translate.rs の `each_reference_to_an_extern_as_a_value_gets_its_own_wrapper`、`names_outside_the_entry_are_qualified_with_their_module`、`builtins_used_as_values_are_wrapped`、`lambdas_are_lifted_with_their_captures_first`、`a_masked_callback_call`、`a_saturated_known_call_takes_the_mask_of_its_last_arrow`、`extra_arguments_of_a_known_call_take_the_mask_of_their_arrow`、`a_continuation_call_in_its_clause_has_no_mask`、perceus.rs の `a_nested_pattern_gives_up_the_parent_before_the_release`。差分が「関数の名前に `@[…]` が付く、型変数の位置の Repr が具体的になる、それに伴って `box` と `unbox` が増減する」だけであることを確かめてから受け入れる。

`function(…, "run")` のように名前で関数を引くテストは、名前の引数を instance の名前 (`"run@[Int]"` など) に直す。`externs_are_called_by_their_canonical_name` の `contains` の文字列は、instance の名前を含む形に直す。

上に挙げた以外のテストの期待値が変わったら、spec の範囲 (「`eml_core_ir` のテストのうち、型変数を持つ関数を具体的な型で使うプログラムのダンプ、`function` に渡す名前、`contains` で調べる文字列」) に入るかを確かめる。入るなら受け入れて台帳に書き、入らないなら止めて報告する。

- [ ] **Step 8: boxing.rs の2件を置き換える**

`int_constants_are_boxed_and_other_constants_pass_as_they_are` を、データの型変数のフィールドで確かめる形に置き換える (成否の変更)。

```rust
#[test]
fn int_constants_are_boxed_and_other_constants_pass_as_they_are() {
    // 型変数のフィールドは `tobj` なので、`int` の定数は `box` を通り、`unit`、タグ、関数の値はそのまま入る
    let text = "data Four a b c d =\n  | Four a b c d\n\ncount : Four a b c d -> Int\ncount _ = 0\n\nid : a -> a\nid x = x\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (count (Four 5 () True id)))";
    insta::assert_snapshot!(function(&core_text(text, Pass::Boxing), "main"), @"");
}
```

`a_unit_value_passes_to_tobj_without_an_instruction` も同じく置き換える。

```rust
#[test]
fn a_unit_value_passes_to_tobj_without_an_instruction() {
    let text = "data Box a =\n  | Box a\n\nwrap : Unit -> Box Unit\nwrap u = Box u\n\nmain : Unit -> <IO> Unit\nmain () =\n  let _ = wrap ()\n  println \"done\"";
    insta::assert_snapshot!(function(&core_text(text, Pass::Boxing), "wrap"), @"");
}
```

Run: `cargo insta test -p eml_core_ir --accept -- boxing::int_constants_are_boxed boxing::a_unit_value_passes`

受け入れたスナップショットを読み、1件目の `main` で `box 5` が1つだけあり、`()`、`#1`、関数の値 (`&"id@[_]"` か `&"id@[…]"`) が `box` なしで `con` に渡っていること、2件目の `wrap` で `u.0` が命令なしで `con` に渡っていることを確かめる。違えば止めて報告する。

- [ ] **Step 9: UI テストを足す**

`tests/ui/run/functions/polymorphic_recursion.em`:

```haskell
-- Polymorphic recursion: depth grows its type argument and runs on the uniform instance;
-- walk grows only `a`, so `b` is still specialized to Int.
depth : Int -> a -> Int
depth n x = if n == 0 then 0 else 1 + depth (n - 1) (x, x)

walk : Int -> a -> b -> b
walk n x y = if n == 0 then y else walk (n - 1) (x, x) y

main : Unit -> <IO> Unit
main () =
  println (show_int (depth 10 1))
  println (show_int (walk 10 "x" 42))
```

Run: `cargo insta test -p eml_cli --accept -- ui::`
Expected: 新しいスナップショットが1つでき、出力が `10` と `42` の2行である。既存の UI のスナップショットは変わらない (`git status --short crates/eml_cli/tests/snapshots/` が新しいファイル1つだけを出す)

- [ ] **Step 10: 基準のプログラムの回数を受け入れる**

Run: `cargo insta test -p eml_interp --accept -- bench::` の後、`git diff --no-ext-diff crates/eml_interp/tests/bench.rs`

差分を読み、`list` と `tree` の `boxes` と `unboxes` の変化を控える (減るはずだが、spec のとおり減る量は約束しない)。`empty`、`fib`、`loop` の回数が変わったら、止めて理由を調べる (型変数を持たない関数しかないので、変わらないはずである)。

- [ ] **Step 11: 全体を確かめる**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check && cargo test -p eml_cli --test integration citations`
Expected: すべて通る

- [ ] **Step 12: コミット**

```bash
git add crates/eml_core_ir/src/translate/instances.rs crates/eml_core_ir/tests/instances.rs crates/eml_core_ir/tests/main.rs crates/eml_core_ir/tests/translate.rs crates/eml_core_ir/tests/boxing.rs crates/eml_core_ir/tests/perceus.rs crates/eml_interp/tests/bench.rs tests/ui/run/functions/polymorphic_recursion.em crates/eml_cli/tests/snapshots/
git commit
```

コミットメッセージは「Monomorphize function code per instance」を件名にし、本文に次を書く: 期待値の変更 (変わったテストの名前と、名前と Repr と `box`/`unbox` が変わった理由)、boxing.rs の2件の置き換え (成否の変更とその理由)、`bench.rs` の `list` と `tree` の `boxes` と `unboxes` の前後の値。末尾に Global Constraints の2行を付ける。

---

### Task 5: コンパイル時間のテストと S4b の記録

**Files:**
- Modify: `crates/eml_core_ir/tests/instances.rs`
- Modify: `docs/implementation/benchmarks.md`

- [ ] **Step 1: コンパイル時間のテストを書く**

`crates/eml_core_ir/tests/instances.rs` の末尾に足す。

```rust
const SMALL: usize = 2000;
const MAX_RATIO: f64 = 6.0;
/// 型の深さが大きさに比例する形を測るスレッドのスタック (crates/eml_types/tests/scaling.rs の `DEEP_STACK` と同じ)。
const DEEP_STACK: usize = 64 << 20;

/// 多相な関数の鎖を `Int` と `String` で使う形。
fn chain(n: usize) -> String {
    let mut text = String::new();
    for i in 1..n {
        text.push_str(&format!("f{i} : a -> a\nf{i} x = f{} x\n\n", i + 1));
    }
    text.push_str(&format!("f{n} : a -> a\nf{n} x = x\n\n"));
    text.push_str("main : Unit -> <IO> Unit\nmain () = println (show_int (f1 1) ++ f1 \"s\")");
    text
}

/// 部分を共有する型を作る `let` の列を、総称な関数の本体に置く形。`ident` の型引数は列の長さの指数の大きさで表示され、
/// `run` の型変数を含む。
fn shared_types(n: usize) -> String {
    let mut text = String::from("ident : a -> a\nident x = x\n\nrun : a -> a\nrun x =\n  let f0 = ident\n");
    for i in 1..=n {
        text.push_str(&format!("  let f{i} = f{} ident\n", i - 1));
    }
    text.push_str(&format!("  f{n} x\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (run 5))"));
    text
}

/// Core IR の段階 (`eml_core_ir::lower`) だけの時間。3回測って最小を使う。
fn lower_time(text: &str) -> std::time::Duration {
    let checked = eml_test_support::check(text);
    assert!(checked.diagnostics.is_empty(), "the generated program has errors");
    let (entry, _) = checked
        .program
        .functions()
        .find(|(id, function)| checked.program.modules[id.module].name == "Main" && function.name == "main")
        .unwrap();
    (0..3)
        .map(|_| {
            let start = std::time::Instant::now();
            let _ = eml_core_ir::lower(&checked.program, &checked.typed, entry, checked.files());
            start.elapsed()
        })
        .min()
        .unwrap()
}

fn assert_linear(generate: fn(usize) -> String) {
    let small = lower_time(&generate(SMALL));
    let large = lower_time(&generate(SMALL * 4));
    let ratio = large.as_secs_f64() / small.as_secs_f64();
    assert!(
        ratio <= MAX_RATIO,
        "size {SMALL} took {small:?} and size {} took {large:?} (ratio {ratio:.1})",
        SMALL * 4
    );
}

fn assert_linear_deep(generate: fn(usize) -> String) {
    let measured = std::thread::Builder::new()
        .stack_size(DEEP_STACK)
        .spawn(move || assert_linear(generate))
        .unwrap()
        .join();
    if let Err(panic) = measured {
        std::panic::resume_unwind(panic);
    }
}

#[test]
#[ignore = "release ビルドで時間を測る"]
fn lowering_a_chain_of_polymorphic_functions_is_linear() {
    assert_linear(chain);
}

#[test]
#[ignore = "release ビルドで時間を測る"]
fn lowering_shared_type_arguments_is_linear() {
    assert_linear_deep(shared_types);
}
```

入口のモジュールは、`crates/eml_types/tests/instantiations.rs` の `records` と同じく、モジュールの名前 (`Main`) で選ぶ。

- [ ] **Step 2: 測る**

Run: `cargo test --release -p eml_core_ir --test integration instances::lowering -- --ignored`
Expected: 2つとも PASS。落ちたら、比と時間を控え、どの処理が大きさの2乗以上で伸びているかを調べる (`substitute` の覚え方、`vars_in` の覚え方、名前の上限)

- [ ] **Step 3: S4b の記録を足す**

Run: `bench/run.sh` (コミットしていない変更がない木で流す。出力の「コミット」に `-dirty` が付いていないことを確かめる)

`docs/implementation/benchmarks.md` の「記録」に「### S4b (単相化)」を足し、出力の環境と表を貼る。表の後に、Task 4 の Step 10 で控えた `list` と `tree` の `boxes` と `unboxes` の前後の値を1段落で書く (「回数のテスト」のスナップショットの差分であることも書く)。

- [ ] **Step 4: 全体を確かめる**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check && cargo test -p eml_cli --test integration citations`
Expected: すべて通る

- [ ] **Step 5: コミット**

```bash
git add crates/eml_core_ir/tests/instances.rs docs/implementation/benchmarks.md
git commit -m "Add lowering-time scaling tests and record the S4b benchmarks"
```

(メッセージの末尾に Global Constraints の2行を付ける)

---

### Task 6: 文書

**Files:**
- Modify: `docs/spec/core-ir.md`、`docs/implementation/architecture.md`、`docs/implementation/testing.md`、`docs/implementation/status.md`、`docs/implementation/benchmarks.md`、`docs/overview.md`、`docs/future/roadmap.md`、`docs/README.md`、`CLAUDE.md`
- Modify: 自分の変更で正しくなくなったソースのコメント (`grep -rn "S4b\|読むだけ\|型を作らない\|reachable" crates` で探す)

spec の「文書」の各項目を、そのとおりに直す。書く中身は spec の「`eml_types` の変更」と「translate の instance の表」の節から写し、規範の文 (「〜にする」「〜である」) の形に直す。下の各 Step の「書く中身」は、最低限入れる事項である。

- [ ] **Step 1: `docs/spec/core-ir.md`**

- 「変換の規則」の「変換は、入口の関数から届く関数だけを Core IR にする。…」の項目を、次の事項を持つ項目の列に替える: instance の鍵 (関数と、シグネチャの型変数の順の型引数)、鍵の作り方 (参照の型引数に instance の代入をかけ、一様な位置を `Flexible` にする)、一様な位置の決め方 (型変数の流れのグラフ、大きくなる辺、大きくなる成分、自分自身への辺)、入口の鍵、instance を集める順 (FIFO、式の ID の順) と番号の順 (HIR の関数の順、見つけた順、`TypeId` の値には依らない)、名前 (`@[…]`、`_`、64 文字を超えたら `@N`、重なったら後を `@N`)、節の型変数は `Flexible` になること、データの配置と `op$`、`con$`、`cont$` はスキームから作ること
- 「位置の規則」の「トップレベルの関数と `op$`、`con$`、`$externN` はスキームから」を、「トップレベルの関数は instance の代入をかけた宣言の型から、`op$`、`con$`、`$externN` はスキームから」にする
- 「一様な関数」の `f$boxed` の根拠を、spec の「`f$boxed` の規則の根拠」のとおりに書き直す

- [ ] **Step 2: `docs/implementation/architecture.md`**

- 「`eml_types` の内部」: `OpVar` (節の操作ごとの型変数、表示は名前、推論は変えない)、`Substitution` と `substitute` (覚え方、`OpVar` を `Flexible`、row の末尾は変えない)、型変数を含むかの印、`display_bounded`、型の表の決まり (後の段階は `substitute` でだけ型を足す) を書く。「表の型引数は、rigid な型変数を名前だけで書き出す。…S4b で…決める」の注意書きを、`OpVar` で区別したことの説明に直す。「S4b の単相化、S5 の型クラスの証拠」は「単相化の instance の表、S5 の型クラスの証拠」にする
- 「translate の組み立て」: `translate/instances.rs` (一様な位置、FIFO で集める、代入をかけた本体の型の表、番号の順、名前) と、`BodyCtx` の `targets` と `indices` を書く。持ち上げた関数の名前の説明に、instance の名前を根にすることを足す

- [ ] **Step 3: `docs/implementation/testing.md`**

- 「Core IR のテキストの形」に、関数の名前が字句の区切りを含むときは引用符で囲む (`fn "f@[Int]"(…)`、`&"f@[Int]"`) ことを足す
- 「性能のテスト」に、`crates/eml_core_ir/tests/instances.rs` (instance の数のテストと、`lower` の時間の比のテスト2つ、`#[ignore]`、64 MiB のスタック) を足す
- 「crate の中の置き方」の、段階の API を直接呼ぶテストの列に、instances.rs のコンパイル時間のテストと入口を選ぶテストを足す
- 「よく使うコマンド」に `cargo test --release -p eml_core_ir --test integration instances::lowering -- --ignored` を足す

- [ ] **Step 4: `docs/implementation/status.md`、`benchmarks.md`、`overview.md`**

- status.md の「深さと性能」に2項目を足す: 型を倍々に大きくして呼ぶ関数の鎖では instance の数が指数的に増える (上限は置かない)、鍵は型そのものなので Repr が同じでも別の instance になり同じ形のコードが複数できる (まとめるのは処理系の最適化)
- benchmarks.md の冒頭の「S4b の単相化、S7 の evidence passing、S10 の VM は、ここの記録と比べる」を、S4b を終えた段として読める形にする (「S7 の evidence passing と S10 の VM は、…比べる。S4b の単相化の前後は「記録」にある」など)。`list.em` の説明の「S4b の単相化で減るかを見るために入れてある」を、単相化の前後の記録を指す形にする
- overview.md の「確定した設計判断」の型システムか実行系の表に、「関数のコードは (関数, 型引数) の instance ごとに単相化し、データの配置は一様のままにする」を1行足し、リンク先を `spec/core-ir.md` の「変換の規則」にする

- [ ] **Step 5: ロードマップと README と CLAUDE.md**

- ロードマップ: 段の列から S4b の行を消し、S5 と S7 の前提を「なし」にする。「## S4b 単相化」の節を消す。冒頭の段落の「S4b〜S13」を「S5〜S13」にする。順序の理由の、S4b を主語にした2項目 (先頭に置く理由と、減る量を約束しない理由) を、終えた段として読める形に直す (例: 「単相化 (S4b) を先に終えたのは、…」)。S5 の節の「S4b の instance の表」(2か所) と「関数のコードの単相化は S4b で採る」、S7 の「前提: S4b。」、S9 の「(S4b)」を、[Core IR とインタプリタ](../spec/core-ir.md) の「変換の規則」を指す形か、前提を「なし」にした形に直す。「処理系」の最適化パスの項目に、Repr が同じ instance をまとめることを足す
- README の表の「再設計の段 (S4b〜S13)」を「(S5〜S13)」にする
- CLAUDE.md: Architecture の `TypeStore::intern` is crate-private, so Core IR only reads types を、`TypeStore::intern` is crate-private; later stages add types only through `TypeStore::substitute` (monomorphization) にする。Core IR の段落に、translate は `translate/instances.rs` が入口から集めた (function, type arguments) instance ごとに関数を変換すること (names `f@[Int]`, uniform positions `_`, polymorphic recursion made uniform per type-variable position) を1文で足す

確かめる: `grep -rn "S4b" docs CLAUDE.md --exclude-dir=reports --exclude-dir=superpowers` が、benchmarks.md の記録の見出しと、終えた段として書いた箇所だけを出す。

- [ ] **Step 6: 全体を確かめる**

Run: `cargo test -p eml_cli --test integration citations && cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

- [ ] **Step 7: コミット**

```bash
git add docs/spec/core-ir.md docs/implementation/architecture.md docs/implementation/testing.md docs/implementation/status.md docs/implementation/benchmarks.md docs/overview.md docs/future/roadmap.md docs/README.md CLAUDE.md
git commit -m "Document monomorphization and close S4b in the roadmap"
```

(ソースのコメントを直したら、そのファイルも名前で足す。メッセージの末尾に Global Constraints の2行を付ける)

---

## 段の終わり

すべてのタスクと、ブランチ全体のレビューの指摘を直した後に行う。

- [ ] `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check`、`nix build` が通る
- [ ] spec とこの計画を削除してコミットする

```bash
git rm docs/superpowers/specs/2026-10-10-s4b-monomorphization-design.md docs/superpowers/plans/2026-10-10-s4b-monomorphization.md
git commit -m "Delete the S4b design and plan"
```

(メッセージの末尾に Global Constraints の2行を付ける)
