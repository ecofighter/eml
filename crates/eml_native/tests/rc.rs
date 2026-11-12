//! インラインの参照の数え方 (docs/spec/native.md の「RC」)。差分のテストは、代役のthunkがrunのヒープに作った、数のどの類の
//! 物体にも生成した`dup`と`decref`を流し、その後の物体を、`Heap::dup`と`Heap::decref`で同じ操作をした対の物体と比べる。
//! JITしたコードにエントリのstubを通して入るので、Miriでは流さない。
#![cfg(not(miri))]
use eml_rt::rc::LOCAL_MAX;
use eml_rt::{Ctx, Fast, Word, layout};

use crate::common::native::{Visit, agree, call_visited, native, visit};

/// ヒープの遷移が区別する、数の類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Local1,
    Local2,
    Shared,
    Stuck,
    Immortal,
    Immediate,
}

const CLASSES: [Class; 6] = [
    Class::Local1,
    Class::Local2,
    Class::Shared,
    Class::Stuck,
    Class::Immortal,
    Class::Immediate,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Dup,
    Decref,
}

/// 操作の後の値。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum After {
    /// 生きている物体。数と、`Heap::is_immortal`と`Heap::is_last`の答えである。
    Live {
        rc: i32,
        immortal: bool,
        last: Option<bool>,
    },
    /// 物体は生きている物体の列挙から消えた。`by`は操作の前後で減った生きている物体の数である。
    Freed {
        by: usize,
    },
    Immediate,
}

/// 代役が類1つについて見たもの。生成したコードの後の値と、ヒープの操作の後の対の物体と、ヒープの呼び出しがどれも
/// 成功したかである。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Seen {
    class: Class,
    native: After,
    heap: After,
    ok: bool,
}

/// 代役の記録。代役は`classes`の類ごとに、`seen`に1行を足す。
struct Probe {
    op: Op,
    classes: Vec<Class>,
    seen: Vec<Seen>,
}

const TEXT: &str = "\
fn entry$main() -> unit {
  return ()
}
fn dup_tobj(borrowed x.0: tobj) -> tobj {
  dup x.0
  return x.0
}
fn dup_obj(borrowed x.0: obj) -> obj {
  dup x.0
  return x.0
}
fn decref_tobj(x.0: tobj) -> unit {
  decref x.0
  return ()
}
fn decref_obj(x.0: obj) -> unit {
  decref x.0
  return ()
}
";

/// 物体のヘッダの数の語を指すポインタ。`Word`は`*mut ()`の透明な包みなので、この変換はポインタの出所を保つ。
fn count_word(w: Word) -> *mut i32 {
    // SAFETY: `Word`と`*mut ()`は同じ表現を持つ
    let header = unsafe { std::mem::transmute::<Word, *mut ()>(w) };
    header.wrapping_byte_add(layout::RC_BYTE).cast::<i32>()
}

/// runのヒープの生きている物体 (不死の物体を含む)。
///
/// # Safety
/// 代役の中で、受け取った`ctx`で呼ぶ。
unsafe fn live(ctx: *mut Ctx<Fast>) -> Vec<Word> {
    // SAFETY: 呼ぶ側の約束
    unsafe { (*ctx).heap_mut() }.live_objects()
}

/// 操作の後の`w`。`before`は操作の前の生きている物体の数である。解放された物体は読まない。
///
/// # Safety
/// 代役の中で、受け取った`ctx`で呼ぶ。`w`は即値か、操作の前に生きていた物体である。
unsafe fn after(ctx: *mut Ctx<Fast>, w: Word, before: usize) -> After {
    if !w.is_ptr() {
        return After::Immediate;
    }
    // SAFETY: 呼ぶ側の約束
    let live = unsafe { live(ctx) };
    if !live.contains(&w) {
        return After::Freed {
            by: before - live.len(),
        };
    }
    // SAFETY: `w`はこのヒープの生きている物体である
    unsafe {
        let heap = (*ctx).heap_mut();
        After::Live {
            rc: count_word(w).read(),
            immortal: heap.is_immortal(w),
            last: heap.is_last(w).ok(),
        }
    }
}

/// `class`の物体をrunのヒープに1つ作る。テストは`Local2`と`Shared`の物体に参照を2つ、ほかの物体に1つ持つ。
///
/// # Safety
/// 代役の中で、受け取った`ctx`で呼ぶ。
unsafe fn make(ctx: *mut Ctx<Fast>, class: Class, ok: &mut bool) -> Word {
    // SAFETY: 呼ぶ側の約束
    let heap = unsafe { (*ctx).heap_mut() };
    match class {
        Class::Local1 => heap.alloc_string("one"),
        Class::Local2 => {
            let w = heap.alloc_string("two");
            // SAFETY: `w`はこのヒープの生きている物体である
            *ok &= unsafe { heap.dup(w) }.is_ok();
            w
        }
        Class::Shared => {
            let w = heap.alloc_string("shared");
            // SAFETY: `w`はこのヒープの生きている物体で、ほかの物体を指さない
            unsafe {
                *ok &= heap.dup(w).is_ok();
                *ok &= heap.share(w, 1).is_ok();
            }
            w
        }
        Class::Stuck => {
            let w = heap.alloc_string("stuck");
            // SAFETY: 張り付いた数には2^31 - 2回の`dup`でしか届かないので、テストは数を直接書く。物体はこのヒープの
            // 生きている物体で、テストだけがそれを指す。`LOCAL_MAX`はヒープ自身が作る数で、手放す前に、テストが持つ
            // 参照の数の1に戻す
            unsafe { count_word(w).write(LOCAL_MAX) };
            w
        }
        Class::Immortal => heap.alloc_string_immortal("immortal"),
        Class::Immediate => Word::imm(5),
    }
}

/// テストが`w`に持つ`held`個の参照を手放す。
///
/// # Safety
/// 代役の中で、受け取った`ctx`で呼ぶ。`w`は`make`が作った`class`の値で、テストはそれに`held`個の参照を持つ。
unsafe fn release(ctx: *mut Ctx<Fast>, class: Class, w: Word, held: usize, ok: &mut bool) {
    if class == Class::Stuck {
        // SAFETY: `make`の書いた数を、テストが持つ参照の数に戻す
        unsafe { count_word(w).write(1) };
    }
    // SAFETY: 呼ぶ側の約束
    let heap = unsafe { (*ctx).heap_mut() };
    for _ in 0..held {
        // SAFETY: 呼ぶ側の約束
        *ok &= unsafe { heap.decref(w) }.is_ok();
    }
}

/// 操作の後でテストが持つ参照の数。張り付いた物体は、`release`が数を1に戻すので1つと数える。
fn held(op: Op, class: Class) -> usize {
    match (op, class) {
        (_, Class::Immediate) | (Op::Decref, Class::Local1 | Class::Immortal) => 0,
        (_, Class::Stuck) | (Op::Dup, Class::Immortal) | (Op::Decref, _) => 1,
        (Op::Dup, Class::Local1) => 2,
        (Op::Dup, _) => 3,
    }
}

/// 代役。記録の類ごとに、物体と対の物体を作り、物体を試す関数に渡し、対の物体には同じ操作を`Heap`で行う。`dup`の関数は
/// 引数を借りて新しい参照を返し、`decref`の関数は引数を消費する。
unsafe extern "C" fn each(ctx: *mut Ctx<Fast>, entry: *const u8, _argv: *mut u64) {
    // SAFETY: `visit`が`entry`に記録を置き、この代役だけがそれを使う
    let visit = unsafe { &mut *entry.cast_mut().cast::<Visit<Probe>>() };
    let op = visit.record.op;
    for class in visit.record.classes.clone() {
        let mut ok = true;
        // SAFETY: 代役の中で、受け取った`ctx`で呼び、関数の呼び出しをまたいで`ctx`への参照を持たない
        let seen = unsafe {
            let a = make(ctx, class, &mut ok);
            let b = make(ctx, class, &mut ok);
            if op == Op::Decref && class == Class::Immortal {
                // 不死の物体で手放せる参照は、`acquire_immortal`か`dup`で作ったものだけである
                ok &= (*ctx).heap_mut().acquire_immortal(a).is_ok();
                ok &= (*ctx).heap_mut().acquire_immortal(b).is_ok();
            }
            let before = live(ctx).len();
            let mut argv = [0u64];
            argv.as_mut_ptr().cast::<Word>().write(a);
            call_visited(ctx, visit, &mut argv);
            if op == Op::Dup {
                ok &= argv.as_ptr().cast::<Word>().read() == a;
            }
            let native = after(ctx, a, before);
            let before = live(ctx).len();
            ok &= match op {
                Op::Dup => (*ctx).heap_mut().dup(b),
                Op::Decref => (*ctx).heap_mut().decref(b),
            }
            .is_ok();
            let heap = after(ctx, b, before);
            let held = held(op, class);
            release(ctx, class, a, held, &mut ok);
            release(ctx, class, b, held, &mut ok);
            (native, heap)
        };
        visit.record.seen.push(Seen {
            class,
            native: seen.0,
            heap: seen.1,
            ok,
        });
    }
}

/// `TEXT`の関数`name`で、`classes`の類ごとに`op`を試す。runはcensusで終わる。
fn probe(name: &str, op: Op, classes: &[Class]) -> Vec<Seen> {
    let record = Probe {
        op,
        classes: classes.to_vec(),
        seen: Vec::new(),
    };
    let (finished, record) = visit(TEXT, name, each, record);
    finished.unwrap_or_else(|error| panic!("{name}: {error}"));
    record.seen
}

fn live_after(rc: i32, immortal: bool, last: bool) -> After {
    After::Live {
        rc,
        immortal,
        last: Some(last),
    }
}

/// 類ごとの、操作の後の値。
fn expected(op: Op, class: Class) -> After {
    match (op, class) {
        (_, Class::Immediate) => After::Immediate,
        (_, Class::Stuck) => live_after(LOCAL_MAX, false, false),
        (_, Class::Immortal) => live_after(0, true, false),
        (Op::Dup, Class::Local1) => live_after(2, false, false),
        (Op::Dup, Class::Local2) => live_after(3, false, false),
        (Op::Dup, Class::Shared) => live_after(-3, false, false),
        (Op::Decref, Class::Local1) => After::Freed { by: 1 },
        (Op::Decref, Class::Local2) => live_after(1, false, true),
        (Op::Decref, Class::Shared) => live_after(-1, false, true),
    }
}

#[test]
fn inline_dup_and_decref_agree_with_the_heap() {
    // `obj`の語はいつもポインタなので、`obj`の関数には即値を渡さない
    let objects = &CLASSES[..5];
    for (name, op, classes) in [
        ("dup_tobj", Op::Dup, &CLASSES[..]),
        ("dup_obj", Op::Dup, objects),
        ("decref_tobj", Op::Decref, &CLASSES[..]),
        ("decref_obj", Op::Decref, objects),
    ] {
        let want: Vec<Seen> = classes
            .iter()
            .map(|&class| Seen {
                class,
                native: expected(op, class),
                heap: expected(op, class),
                ok: true,
            })
            .collect();
        assert_eq!(probe(name, op, classes), want, "{name}");
    }
}

#[test]
fn a_last_decref_frees() {
    // 局所の物体の最後の参照は、それを持つ関数でも、それを消費する呼ばれた側でも、物体を解放する。数を1のまま残せば
    // censusが漏れを見つける。0を書く誤りは、物体を不死にしてcensusから外すので、差分のテストが見つける。手放しすぎは、
    // 速いモードのネイティブでは解放した物体の使用で、ヒープは誤りにしない。`agree`の検査モードのVMのrunが確かめるのは、
    // LIRの参照の釣り合いである
    let text = "\
fn give(x.0: obj) -> unit {
  decref x.0
  return ()
}
fn entry$main() -> unit {
  let s.0: obj = extern \"Prelude.Show Int.show\"(42)
  dup s.0
  let u.1: unit = extern Prelude.println(s.0)
  decref s.0
  let t.2: obj = extern \"Prelude.Show Int.show\"(43)
  let z.3: unit = call give(t.2)
  return u.1
}
";
    agree(text);
    assert_eq!(native(text), ("42\n".to_string(), Ok(None)));
}

#[test]
fn drop_releases_one_reference() {
    // `drop`はちょうど1つの参照を手放す。1つ目の`drop`の後も文字列は表示でき、2つ目が解放する
    let text = "\
fn entry$main() -> unit {
  let s.0: obj = extern \"Prelude.Show Int.show\"(7)
  dup s.0
  dup s.0
  let d.1: unit = drop s.0
  let u.2: unit = extern Prelude.println(s.0)
  let e.3: unit = drop s.0
  return u.2
}
";
    agree(text);
    assert_eq!(native(text), ("7\n".to_string(), Ok(None)));
}
