//! `release`、token、reuseの`con`、構造体の箱 (docs/spec/native.md の「データ」)。差分のテストは、代役のthunkがrunの
//! ヒープに作った物体を生成したコードに渡し、その後の物体を、`Heap`の操作を通した対の物体と比べる。JITしたコードに
//! エントリのstubを通して入るので、Miriでは流さない。
#![cfg(not(miri))]
use eml_native::Arg;
use eml_rt::rc::LOCAL_MAX;
use eml_rt::{Ctx, Fast, Kind, Token, Word, layout};

use crate::common::native::{Visit, agree, call, call_visited, clif, native, visit};

/// 差分のテストの関数。`rel`は残す欄を返す`release`で、`repl`はtokenを作る`release`と、その場のreuseの`con`である。
/// `repl`は欄0と生の欄を書かず、欄1に引数`c.1`を書く。結果の変数の印は`lin`で、親は`lin`ビットを持たない。その場の
/// reuseはヘッダを書かないのでビットを持たず、確保した物体だけがビットを持つ。空けた親の箱はすぐに確保に使われるので、
/// 箱の番地では2つの道を見分けられないためである
const TEXT: &str = "\
layout P { P(obj, obj, int) }
fn entry$main() -> unit {
  return ()
}
fn rel(x.0: obj) -> obj {
  unpack x.0 P #0(a.1: obj, b.2: obj, n.3: int)
  release x.0 P #0(a.1, _, _)
  return a.1
}
fn repl(x.0: obj, c.1: obj) -> obj lin {
  unpack x.0 P #0(a.2: obj, b.3: obj, n.4: int)
  let r.5: token = release x.0 P #0(a.2, _, _)
  let d.6: obj lin = con P #0(=a.2, c.1, =n.4) reuse r.5 in place
  return d.6
}
";

/// 親の数の類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Unique,
    Local2,
    /// 共有の数-1。最後の参照である。
    SharedLast,
    /// 共有の数-2。
    Shared,
    Stuck,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Form {
    /// `rel`。
    Plain,
    /// `repl`。
    Token,
}

/// 親と2つの子の、操作の後の数。`None`は生きている物体の列挙から消えたこと (解放) である。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct State {
    parent: Option<i32>,
    kept: Option<i32>,
    dropped: Option<i32>,
}

/// `repl`が作った物体。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Built {
    /// 結果の語が親の箱の語であること。`Heap`の側は、tokenの語が箱の語か空の語かを`ok`で確かめる。
    at_parent: bool,
    rc: i32,
    meta: (Kind, u16, u8, bool),
    /// 欄0が残した子の語であること。
    kept_field: bool,
    /// 欄1が新しい欄の語であること。
    new_field: bool,
    raw: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Outcome {
    state: State,
    /// `rel`では、結果が残した子の語であること。
    returns_kept: bool,
    built: Option<Built>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Seen {
    class: Class,
    native: Outcome,
    heap: Outcome,
    ok: bool,
}

struct Probe {
    form: Form,
    classes: Vec<Class>,
    seen: Vec<Seen>,
}

/// 親とその2つの子。
#[derive(Clone, Copy)]
struct Parts {
    parent: Word,
    kept: Word,
    dropped: Word,
}

/// 物体のヘッダの数の語を指すポインタ。`Word`は`*mut ()`の透明な包みなので、この変換はポインタの出所を保つ。
fn count_word(w: Word) -> *mut i32 {
    // SAFETY: `Word`と`*mut ()`は同じ表現を持つ
    let header = unsafe { std::mem::transmute::<Word, *mut ()>(w) };
    header.wrapping_byte_add(layout::RC_BYTE).cast::<i32>()
}

/// `w`が生きていればその数。
///
/// # Safety
/// 代役の中で、受け取った`ctx`で呼ぶ。`w`はこのヒープで作った物体である。
unsafe fn rc_of(ctx: *mut Ctx<Fast>, w: Word) -> Option<i32> {
    // SAFETY: 呼ぶ側の約束。生きている物体の数だけを読む
    unsafe {
        let live = (*ctx).heap_mut().live_objects();
        live.contains(&w).then(|| count_word(w).read())
    }
}

/// `class`の親`P #0(kept, dropped, 3)`をrunのヒープに作る。テストは`Local2`と`Shared`の親に参照を2つ、ほかの親に
/// 1つ持つ。
///
/// # Safety
/// 代役の中で、受け取った`ctx`で呼ぶ。
unsafe fn make(ctx: *mut Ctx<Fast>, class: Class, ok: &mut bool) -> Parts {
    // SAFETY: 呼ぶ側の約束。子の参照は親の欄へ移る
    unsafe {
        let heap = (*ctx).heap_mut();
        let kept = heap.alloc_string("kept");
        let dropped = heap.alloc_string("dropped");
        let parent = heap.alloc_data(0, 2, 1, false);
        *ok &= heap.set_field(parent, 0, kept).is_ok();
        *ok &= heap.set_field(parent, 1, dropped).is_ok();
        *ok &= heap.set_raw(parent, 2, 3).is_ok();
        match class {
            Class::Unique => {}
            Class::Local2 => *ok &= heap.dup(parent).is_ok(),
            Class::SharedLast => *ok &= heap.share(parent, 1).is_ok(),
            Class::Shared => {
                *ok &= heap.dup(parent).is_ok();
                *ok &= heap.share(parent, 1).is_ok();
            }
            // 張り付いた数には2^31 - 2回の`dup`でしか届かないので、数を直接書く。手放す前に1に戻す
            Class::Stuck => count_word(parent).write(LOCAL_MAX),
        }
        Parts {
            parent,
            kept,
            dropped,
        }
    }
}

/// 操作の後の親と子の数。
///
/// # Safety
/// 代役の中で、受け取った`ctx`で呼ぶ。
unsafe fn state(ctx: *mut Ctx<Fast>, parts: Parts) -> State {
    // SAFETY: 呼ぶ側の約束
    unsafe {
        State {
            parent: rc_of(ctx, parts.parent),
            kept: rc_of(ctx, parts.kept),
            dropped: rc_of(ctx, parts.dropped),
        }
    }
}

/// `repl`の結果`w`の見え方。`fresh`は欄1に書いた新しい欄である。
///
/// # Safety
/// 代役の中で、受け取った`ctx`で呼ぶ。`w`はこのヒープの生きている`P #0`である。
unsafe fn built(ctx: *mut Ctx<Fast>, w: Word, parts: Parts, fresh: Word, ok: &mut bool) -> Built {
    // SAFETY: 呼ぶ側の約束
    unsafe {
        let heap = (*ctx).heap_mut();
        let meta = heap.meta(w).map(|m| (m.kind(), m.tag(), m.scan(), m.lin()));
        let (f0, f1, raw) = (heap.field(w, 0), heap.field(w, 1), heap.raw(w, 2));
        *ok &= meta.is_ok() && f0.is_ok() && f1.is_ok() && raw.is_ok();
        Built {
            at_parent: w == parts.parent,
            rc: count_word(w).read(),
            meta: meta.unwrap_or((Kind::Free, 0, 0, false)),
            kept_field: f0 == Ok(parts.kept),
            new_field: f1 == Ok(fresh),
            raw: raw.unwrap_or(0),
        }
    }
}

/// 生成したコードの側。親`parts`を試す関数に渡す。
///
/// # Safety
/// 代役の中で、受け取った`ctx`で呼ぶ。テストは`parts.parent`に参照を持つ。
unsafe fn on_native(
    ctx: *mut Ctx<Fast>,
    visit: &Visit<Probe>,
    parts: Parts,
    ok: &mut bool,
) -> (Outcome, Word) {
    // SAFETY: 呼ぶ側の約束。関数の呼び出しをまたいで`ctx`への参照を持たない
    unsafe {
        match visit.record.form {
            Form::Plain => {
                let mut argv = [0u64];
                argv.as_mut_ptr().cast::<Word>().write(parts.parent);
                call_visited(ctx, visit, &mut argv);
                let r = argv.as_ptr().cast::<Word>().read();
                let outcome = Outcome {
                    state: state(ctx, parts),
                    returns_kept: r == parts.kept,
                    built: None,
                };
                (outcome, r)
            }
            Form::Token => {
                let fresh = (*ctx).heap_mut().alloc_string("fresh");
                let mut argv = [0u64; 2];
                argv.as_mut_ptr().cast::<Word>().write(parts.parent);
                argv.as_mut_ptr().cast::<Word>().add(1).write(fresh);
                call_visited(ctx, visit, &mut argv);
                let r = argv.as_ptr().cast::<Word>().read();
                let outcome = Outcome {
                    state: state(ctx, parts),
                    returns_kept: false,
                    built: Some(built(ctx, r, parts, fresh, ok)),
                };
                (outcome, r)
            }
        }
    }
}

/// `Heap`の側。生成したコードと同じ操作を`Heap`の部品で行う。一意な親のtokenは`Heap::into_token`で作り、
/// `Heap::reuse_in_place`で使う。ほかの親は`Heap::release_reuse`が空のtokenを返すので、新しく確保する。
///
/// # Safety
/// 代役の中で、受け取った`ctx`で呼ぶ。テストは`parts.parent`に参照を持つ。
unsafe fn on_heap(
    ctx: *mut Ctx<Fast>,
    form: Form,
    class: Class,
    parts: Parts,
    ok: &mut bool,
) -> (Outcome, Word) {
    let w = parts.parent;
    // SAFETY: 呼ぶ側の約束。`P #0`は走査する語を2つ持ち、欄0を残す
    unsafe {
        let heap = (*ctx).heap_mut();
        match form {
            Form::Plain => {
                *ok &= heap.release(w, 0, &[true, false]).is_ok();
                let outcome = Outcome {
                    state: state(ctx, parts),
                    returns_kept: true,
                    built: None,
                };
                (outcome, parts.kept)
            }
            Form::Token => {
                let fresh = heap.alloc_string("fresh");
                let r = if class == Class::Unique {
                    *ok &= heap.decref(parts.dropped).is_ok();
                    let token = heap.into_token(w).map(Token::into_word);
                    *ok &= token == Ok(w);
                    // tokenは`into_word`が返した語である
                    let token = Token::from_word(token.unwrap_or(Word::EMPTY));
                    let r = heap.reuse_in_place(token, layout::data_meta(0, 2, true));
                    *ok &= r == Ok(w);
                    let r = r.unwrap_or(w);
                    *ok &= heap.set_field(r, 1, fresh).is_ok();
                    r
                } else {
                    let token = heap.release_reuse(w, 0, &[true, false]);
                    *ok &= token.as_ref().is_ok_and(|t| t.is_empty());
                    let r = heap.alloc_data(0, 2, 1, true);
                    *ok &= heap.set_field(r, 0, parts.kept).is_ok();
                    *ok &= heap.set_field(r, 1, fresh).is_ok();
                    *ok &= heap.set_raw(r, 2, 3).is_ok();
                    r
                };
                let outcome = Outcome {
                    state: state(ctx, parts),
                    returns_kept: false,
                    built: Some(built(ctx, r, parts, fresh, ok)),
                };
                (outcome, r)
            }
        }
    }
}

/// テストが持つ参照を手放す。`result`は試した操作の結果で、テストがその参照を1つ持つ。
///
/// # Safety
/// 代役の中で、受け取った`ctx`で呼ぶ。
unsafe fn finish(
    ctx: *mut Ctx<Fast>,
    form: Form,
    class: Class,
    parts: Parts,
    result: Word,
    ok: &mut bool,
) {
    // SAFETY: 呼ぶ側の約束
    unsafe {
        let heap = (*ctx).heap_mut();
        let parent_refs = match (form, class) {
            (_, Class::SharedLast) | (Form::Plain, Class::Unique) => 0,
            // その場のreuseでは、結果が親の箱そのものである
            (Form::Token, Class::Unique) => 0,
            _ => 1,
        };
        if class == Class::Stuck {
            count_word(parts.parent).write(1);
        }
        for _ in 0..parent_refs {
            *ok &= heap.decref(parts.parent).is_ok();
        }
        *ok &= heap.decref(result).is_ok();
    }
}

/// 代役。記録の類ごとに親と対の親を作り、親を試す関数に、対の親を`Heap`の操作に通す。
unsafe fn each(ctx: *mut Ctx<Fast>, visit: &mut Visit<Probe>) {
    let form = visit.record.form;
    for class in visit.record.classes.clone() {
        let mut ok = true;
        // SAFETY: 代役の中で、受け取った`ctx`で呼び、関数の呼び出しをまたいで`ctx`への参照を持たない
        let (native, heap) = unsafe {
            let a = make(ctx, class, &mut ok);
            let b = make(ctx, class, &mut ok);
            let (native, ra) = on_native(ctx, visit, a, &mut ok);
            let (heap, rb) = on_heap(ctx, form, class, b, &mut ok);
            finish(ctx, form, class, a, ra, &mut ok);
            finish(ctx, form, class, b, rb, &mut ok);
            (native, heap)
        };
        visit.record.seen.push(Seen {
            class,
            native,
            heap,
            ok,
        });
    }
}

fn probe(form: Form, classes: &[Class]) -> Vec<Seen> {
    let name = match form {
        Form::Plain => "rel",
        Form::Token => "repl",
    };
    let record = Probe {
        form,
        classes: classes.to_vec(),
        seen: Vec::new(),
    };
    let (finished, record) = visit(TEXT, name, each, record);
    finished.unwrap_or_else(|error| panic!("{name}: {error}"));
    record.seen
}

/// 類ごとの、操作の後の親と子の数。
fn expected_state(form: Form, class: Class) -> State {
    let (parent, kept, dropped) = match (form, class) {
        (Form::Plain, Class::Unique) => (None, Some(1), None),
        (Form::Token, Class::Unique) => (Some(1), Some(1), None),
        (_, Class::Local2) => (Some(1), Some(2), Some(1)),
        (Form::Plain, Class::SharedLast) => (None, Some(-1), None),
        // 空けた親の箱は同じ級の空いたブロックとして、`con`の確保がすぐに受け取る。親の数の語は結果の数である
        (Form::Token, Class::SharedLast) => (Some(1), Some(-1), None),
        (_, Class::Shared) => (Some(-1), Some(-2), Some(-1)),
        (_, Class::Stuck) => (Some(LOCAL_MAX), Some(2), Some(1)),
    };
    State {
        parent,
        kept,
        dropped,
    }
}

fn expected(form: Form, class: Class) -> Outcome {
    // 共有の最後の参照の結果は、空けた親の箱に確保した新しい物体である。空のtokenで確保したことは、`lin`ビットと数1に
    // 現れる。その場のreuseはヘッダを書かないので、ビットはなく、数は共有の数のまま残る
    let built = (form == Form::Token).then_some(Built {
        at_parent: matches!(class, Class::Unique | Class::SharedLast),
        rc: 1,
        meta: (Kind::Data, 0, 2, class != Class::Unique),
        kept_field: true,
        new_field: true,
        raw: 3,
    });
    Outcome {
        state: expected_state(form, class),
        returns_kept: form == Form::Plain,
        built,
    }
}

fn differential(form: Form, classes: &[Class]) {
    let want: Vec<Seen> = classes
        .iter()
        .map(|&class| Seen {
            class,
            native: expected(form, class),
            heap: expected(form, class),
            ok: true,
        })
        .collect();
    assert_eq!(probe(form, classes), want);
}

/// 表示の補助。`n`を表示する。
const SHOW: &str = "\
fn show(n.0: int) -> unit {
  let s.1: obj = extern \"Prelude.Show Int.show\"(n.0)
  let u.2: unit = extern Prelude.println(s.1)
  return u.2
}
fn print(borrowed s.0: obj) -> unit {
  dup s.0
  let u.1: unit = extern Prelude.println(s.0)
  return u.1
}
";

/// `P`の値の欄を表示し、値を手放す。
const DUMP: &str = "\
fn dump(p.0: obj) -> unit {
  unpack p.0 P #0(a.1: obj, b.2: obj, n.3: int)
  let u.4: unit = call print(a.1)
  let v.5: unit = call print(b.2)
  let w.6: unit = call show(n.3)
  decref p.0
  return w.6
}
";

/// `P #0(show a, show b, n)`を作る文。`p.<base>`に束縛し、`base + 1`と`base + 2`も使う。
fn make_p(base: u32, a: i64, b: i64, n: i64) -> String {
    format!(
        "  let s.{s}: obj = extern \"Prelude.Show Int.show\"({a})\n  \
         let t.{t}: obj = extern \"Prelude.Show Int.show\"({b})\n  \
         let p.{base}: obj = con P #0(s.{s}, t.{t}, {n})\n",
        s = base + 1,
        t = base + 2,
    )
}

#[test]
fn release_frees_a_last_reference_and_drops_children() {
    // 捨てる子と親が残れば、censusが漏れを見つける
    let text = format!(
        "\
layout P {{ P(obj, obj, int) }}
{SHOW}fn take(x.0: obj) -> obj {{
  unpack x.0 P #0(a.1: obj, b.2: obj, n.3: int)
  release x.0 P #0(a.1, _, _)
  return a.1
}}
fn entry$main() -> unit {{
{make}  let a.10: obj = call take(p.0)
  let u.11: unit = call print(a.10)
  decref a.10
  return u.11
}}
",
        make = make_p(0, 1, 2, 3)
    );
    agree(&text);
    assert_eq!(native(&text), ("1\n".to_string(), Ok(None)));
    differential(Form::Plain, &[Class::Unique]);
}

#[test]
fn release_of_a_shared_value_dups_the_kept_fields() {
    // 一意でない親では、残す子を`dup`して親を手放す。共有の最後の参照 (-1) は、局所の1と同じく子を受け取って解放する
    differential(
        Form::Plain,
        &[
            Class::Local2,
            Class::SharedLast,
            Class::Shared,
            Class::Stuck,
        ],
    );
}

#[test]
fn inline_token_moves_agree_with_the_heap() {
    // 速いモードのtokenは箱の語で、その場のreuseはtokenの語をそのまま箱にする。`Heap::into_token`と
    // `Heap::reuse_in_place`と同じく、数、`meta`、書かない欄の語を変えない
    differential(Form::Token, &[Class::Unique]);
}

#[test]
fn release_of_a_shared_or_stuck_value_gives_an_empty_token() {
    differential(
        Form::Token,
        &[
            Class::Local2,
            Class::SharedLast,
            Class::Shared,
            Class::Stuck,
        ],
    );
}

/// 欄0と欄1を入れ替える、その場のreuse。
const SWAP: &str = "\
fn swap(x.0: obj) -> obj {
  unpack x.0 P #0(a.1: obj, b.2: obj, n.3: int)
  let r.4: token = release x.0 P #0(a.1, b.2, _)
  let d.5: obj = con P #0(b.2, a.1, =n.3) reuse r.4 in place
  return d.5
}
";

#[test]
fn a_token_is_reused_in_place() {
    let text = format!(
        "\
layout P {{ P(obj, obj, int) }}
{SHOW}{DUMP}{SWAP}fn entry$main() -> unit {{
{make}  let q.10: obj = call swap(p.0)
  let u.11: unit = call dump(q.10)
  return u.11
}}
",
        make = make_p(0, 1, 2, 3)
    );
    agree(&text);
    assert_eq!(native(&text), ("2\n1\n3\n".to_string(), Ok(None)));
}

#[test]
fn an_in_place_reuse_calls_no_entry() {
    // その場のreuseはtokenの語を箱にし、一意な`release`は箱の語をtokenにする。どちらもエントリを呼ばないので、関数が
    // 名指すヒープの確保と書き直しのエントリは、空のtokenの道の`eml_rt_alloc_data`だけである
    let clif = clif(TEXT, "repl");
    let called: Vec<&str> = clif
        .lines()
        .filter_map(|line| line.trim().strip_prefix("fn")?.split(" = ").nth(1))
        .filter_map(|callee| callee.split_whitespace().find(|word| word.starts_with('%')))
        .collect();
    let building: Vec<&str> = called
        .iter()
        .copied()
        .filter(|callee| {
            callee.starts_with("%eml_rt_alloc")
                || callee.starts_with("%eml_rt_con")
                || callee.contains("token")
                || callee.contains("reuse")
        })
        .collect();
    assert_eq!(building, ["%eml_rt_alloc_data"], "{clif}");
}

#[test]
fn a_token_of_another_constructor_is_rewritten() {
    // 別の構築子のtokenは`eml_rt_con_at`でヘッダを書き直す。書き直さなければ、2回目の`switch`が元のタグへ進む
    let text = format!(
        "\
layout T {{ A(obj, int), B(int, obj) }}
{SHOW}fn flip(x.0: obj) -> obj {{
  switch x.0 T {{ #0(a.1: obj, n.2: int) -> b1, #1(m.3: int, c.4: obj) -> b2 }}
b1:
  let r.5: token = release x.0 T #0(a.1, _)
  let y.6: obj = con T #1(n.2, a.1) reuse r.5
  return y.6
b2:
  let r.7: token = release x.0 T #1(_, c.4)
  let y.8: obj = con T #0(c.4, m.3) reuse r.7
  return y.8
}}
fn tell(borrowed q.0: obj) -> unit {{
  switch q.0 T {{ #0(a.1: obj, n.2: int) -> b1, #1(m.3: int, c.4: obj) -> b2 }}
b1:
  let u.5: unit = call print(a.1)
  let v.6: unit = call show(n.2)
  return v.6
b2:
  let u.7: unit = call show(m.3)
  let v.8: unit = call print(c.4)
  return v.8
}}
fn entry$main() -> unit {{
  let s.0: obj = extern \"Prelude.Show Int.show\"(7)
  let p.1: obj = con T #0(s.0, 5)
  let q.2: obj = call flip(p.1)
  let u.3: unit = call tell(q.2)
  let r.4: obj = call flip(q.2)
  let v.5: unit = call tell(r.4)
  decref r.4
  return v.5
}}
"
    );
    agree(&text);
    assert_eq!(native(&text), ("5\n7\n7\n5\n".to_string(), Ok(None)));
}

#[test]
fn an_empty_token_allocates() {
    // 参照を2つ持つ値の`release`は空のtokenを作るので、`con`は確保し、元の値は変わらない
    let text = format!(
        "\
layout P {{ P(obj, obj, int) }}
{SHOW}{DUMP}{SWAP}fn entry$main() -> unit {{
{make}  dup p.0
  let q.10: obj = call swap(p.0)
  let u.11: unit = call dump(q.10)
  let v.12: unit = call dump(p.0)
  return v.12
}}
",
        make = make_p(0, 1, 2, 3)
    );
    agree(&text);
    assert_eq!(native(&text), ("2\n1\n3\n1\n2\n3\n".to_string(), Ok(None)));
}

#[test]
fn skipped_fields_keep_their_words() {
    // 書かない欄は、reuseした箱では元の語のまま残り、空のtokenで確保した物体にはほかの欄と同じく書く
    let text = format!(
        "\
layout P {{ P(obj, obj, int) }}
{SHOW}{DUMP}fn repl(x.0: obj, c.1: obj) -> obj {{
  unpack x.0 P #0(a.2: obj, b.3: obj, n.4: int)
  let r.5: token = release x.0 P #0(a.2, _, _)
  let d.6: obj = con P #0(=a.2, c.1, =n.4) reuse r.5 in place
  return d.6
}}
fn entry$main() -> unit {{
{make}  let c.10: obj = extern \"Prelude.Show Int.show\"(9)
  let q.11: obj = call repl(p.0, c.10)
  let u.12: unit = call dump(q.11)
{again}  dup p.20
  let c.24: obj = extern \"Prelude.Show Int.show\"(8)
  let q.25: obj = call repl(p.20, c.24)
  let v.26: unit = call dump(q.25)
  let w.27: unit = call dump(p.20)
  return w.27
}}
",
        make = make_p(0, 1, 2, 3),
        again = make_p(20, 4, 5, 6)
    );
    agree(&text);
    assert_eq!(
        native(&text),
        ("1\n9\n3\n4\n8\n6\n4\n5\n6\n".to_string(), Ok(None))
    );
}

#[test]
fn drop_token_frees_the_box() {
    // 使わなかったtokenの箱が残れば、censusが漏れを見つける。空のtokenを捨てても何も起きない
    let text = format!(
        "\
layout P {{ P(obj, obj, int) }}
{SHOW}{DUMP}fn first(x.0: obj) -> obj {{
  unpack x.0 P #0(a.1: obj, b.2: obj, n.3: int)
  let r.4: token = release x.0 P #0(a.1, _, _)
  drop_token r.4
  return a.1
}}
fn entry$main() -> unit {{
{make}  let a.10: obj = call first(p.0)
  let u.11: unit = call print(a.10)
  decref a.10
{again}  dup p.20
  let b.24: obj = call first(p.20)
  let v.25: unit = call print(b.24)
  decref b.24
  let w.26: unit = call dump(p.20)
  return w.26
}}
",
        make = make_p(0, 1, 2, 3),
        again = make_p(20, 4, 5, 6)
    );
    agree(&text);
    assert_eq!(native(&text), ("1\n4\n4\n5\n6\n".to_string(), Ok(None)));
}

#[test]
fn unbox_struct_reads_leaves() {
    let text = "\
layout V { V(float, obj, int, enum) }
fn entry$main() -> unit {
  return ()
}
fn f(x.0: float, n.1: int, c.2: enum) -> (float, enum, enum) {
  let s.3: obj = extern \"Prelude.Show Int.show\"(n.1)
  let w.4: obj = con V #0(x.0, s.3, n.1, c.2)
  let (y.5: float, t.6: obj, _, d.7: enum) = unbox w.4 V
  let u.8: obj = extern \"Prelude.Show Int.show\"(n.1)
  let e.9: enum = extern \"Prelude.Eq String.==\"(t.6, u.8)
  decref u.8
  decref w.4
  return (y.5, d.7, e.9)
}
";
    for (n, x, c) in [(0, 0.0, 0), (-7, -2.5, 1), (i64::MIN, f64::INFINITY, 2)] {
        let got = call(text, "f", &[Arg::F64(x), Arg::I64(n), Arg::I64(c)]);
        let [Arg::F64(y), d, e] = got[..] else {
            panic!("{got:?}")
        };
        assert_eq!(y.to_bits(), x.to_bits());
        assert_eq!([d, e], [Arg::I64(c), Arg::I64(1)]);
    }
}

#[test]
fn release_unboxed_releases() {
    // 構造体の箱の`release unboxed`は、ダミーでなければふつうの`release`と同じに、tokenを作る形も含めて箱を手放す
    let text = format!(
        "\
layout V {{ V(float, obj, int) }}
{SHOW}fn entry$main() -> unit {{
  let s.0: obj = extern \"Prelude.Show Int.show\"(5)
  let w.1: obj = con V #0(1.5, s.0, 7)
  let (x.2: float, t.3: obj, n.4: int) = unbox w.1 V
  release unboxed w.1 V #0(_, t.3, _)
  let u.5: unit = call print(t.3)
  decref t.3
  let v.6: unit = call show(n.4)
  let s.7: obj = extern \"Prelude.Show Int.show\"(6)
  let w.8: obj = con V #0(2.5, s.7, 8)
  let (_, t.9: obj, m.10: int) = unbox w.8 V
  let u.11: unit = call print(t.9)
  let r.12: token = release unboxed w.8 V #0(_, _, _)
  drop_token r.12
  let v.13: unit = call show(m.10)
  return v.13
}}
"
    );
    agree(&text);
    assert_eq!(native(&text), ("5\n7\n6\n8\n".to_string(), Ok(None)));
}

/// 構造体の箱を受ける関数。`leaves`は借りた箱の葉を返し、`gone`は箱を`release unboxed`で手放す。
const DUMMY: &str = "\
layout V { V(float, obj, int) }
fn entry$main() -> unit {
  return ()
}
fn leaves(borrowed w.0: obj) -> (float, obj, int) {
  let (x.1: float, t.2: obj, n.3: int) = unbox w.0 V
  dup t.2
  return (x.1, t.2, n.3)
}
fn gone(w.0: obj) -> unit {
  let (_, t.1: obj, _) = unbox w.0 V
  let r.2: token = release unboxed w.0 V #0(_, t.1, _)
  drop_token r.2
  decref t.1
  return ()
}
";

/// 代役の記録。`leaves`では結果の葉 (`float`のビット列、箱の語か、`int`) と、呼んだ後の箱の数である。`gone`では
/// 呼んだ後に箱が消えたかである。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Dummy {
    leaves: Option<(u64, bool, u64, i32)>,
    freed: Option<bool>,
    ok: bool,
}

/// 代役。`2^62`の`BigInt`の箱 (ダミーと同じ種類の物体) を作り、構造体の箱として試す関数に渡す。
unsafe fn dummy_each(ctx: *mut Ctx<Fast>, visit: &mut Visit<(bool, Dummy)>) {
    let gone = visit.record.0;
    let mut ok = true;
    // SAFETY: 代役の中で、受け取った`ctx`で呼び、関数の呼び出しをまたいで`ctx`への参照を持たない
    let seen = unsafe {
        let b = (*ctx).heap_mut().alloc_int(1 << 62);
        let mut argv = [0u64; 3];
        argv.as_mut_ptr().cast::<Word>().write(b);
        call_visited(ctx, visit, &mut argv);
        if gone {
            Dummy {
                leaves: None,
                freed: Some(rc_of(ctx, b).is_none()),
                ok,
            }
        } else {
            let leaf = argv.as_ptr().cast::<Word>().add(1).read();
            let rc = count_word(b).read();
            ok &= (*ctx).heap_mut().decref(leaf).is_ok();
            ok &= (*ctx).heap_mut().decref(b).is_ok();
            Dummy {
                leaves: Some((argv[0], leaf == b, argv[2], rc)),
                freed: None,
                ok,
            }
        }
    };
    visit.record.1 = seen;
}

#[test]
fn a_big_int_box_counts_as_the_dummy() {
    // ダミーは`BigInt`の物体なので、`BigInt`の箱を受けた構造体の`unbox`は、数える葉に箱そのものを、生の葉に0を置く。
    // `release unboxed`は何もせず空のtokenを作るので、箱の参照は葉の`decref`が手放す
    let (finished, (_, leaves)) = visit(DUMMY, "leaves", dummy_each, (false, Dummy::default()));
    finished.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        leaves,
        Dummy {
            leaves: Some((0, true, 0, 2)),
            freed: None,
            ok: true
        }
    );
    let (finished, (_, gone)) = visit(DUMMY, "gone", dummy_each, (true, Dummy::default()));
    finished.unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        gone,
        Dummy {
            leaves: None,
            freed: Some(true),
            ok: true
        }
    );
}
