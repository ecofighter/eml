//! `eml_rt` の extern の速いモードの計測 (docs/implementation/benchmarks.md)。`bench/rt.sh` が release ビルドで
//! 走らせ、命令の数を測る。`rt_bench` と別のプログラムにするのは、今ある14の計測のコード生成単位の分け方を変えない
//! ためである (docs/implementation/benchmarks.md の「`eml_rt` のインライン化」)。

use std::hint::black_box;
use std::io::{self, Cursor};
use std::path::PathBuf;
use std::sync::Arc;

use eml_rt::externs::TUPLE;
use eml_rt::{Buffering, Control, Ctx, Fast, Host, HostRef, Limits, Opened, StdHost, Stream, Word};

const LONG: usize = 64 * 1024;
const KIB: usize = 1024;
const MIB: usize = 1024 * 1024;

fn main() {
    let case = std::env::args().nth(1).expect(
        "a case: string-eq, string-eq-early, compare, concat-copy, show-int, show-int-long, \
             show-string, show-string-dense, println-block, println-line, read-all, read-all-unsized",
    );
    match case.as_str() {
        "string-eq" => string_eq(false),
        "string-eq-early" => string_eq(true),
        "compare" => compare(),
        "concat-copy" => concat_copy(),
        "show-int" => show_int(7),
        "show-int-long" => show_int(i64::MIN),
        "show-string" => show_string(&"x".repeat(KIB)),
        "show-string-dense" => show_string(&"a\n".repeat(KIB / 2)),
        "println-block" => println(Buffering::Block),
        "println-line" => println(Buffering::Line),
        "read-all" => read_all(true),
        "read-all-unsized" => read_all(false),
        _ => panic!("an unknown case {case}"),
    }
}

fn ctx_with(host: impl Host + 'static) -> Ctx<Fast> {
    Ctx::new(
        Arc::new(Control::new(Limits::default())),
        HostRef::new(Arc::new(host)),
    )
}

fn ctx() -> Ctx<Fast> {
    ctx_with(StdHost::new(
        Box::new(io::sink()),
        Box::new(io::sink()),
        PathBuf::new(),
        Buffering::Block,
    ))
}

fn string(ctx: &mut Ctx<Fast>, s: &str) -> Word {
    // SAFETY: 文字列を1つ確保するだけで、ほかの物体には触れない。
    unsafe { ctx.heap_mut() }.alloc_string(s)
}

fn decref(ctx: &mut Ctx<Fast>, w: Word) {
    // SAFETY: 呼ぶ側は w の参照を1つ持ち、この後 w を読まない。
    unsafe { ctx.heap_mut().decref(w) }.unwrap();
}

/// 参照を1つ足す。extern は引数の参照を受け取って手放すので、同じ文字列を何度も渡すために使う。
fn dup(ctx: &mut Ctx<Fast>, w: Word) {
    // SAFETY: w は呼ぶ側が参照を持つ生きた物体である。
    unsafe { ctx.heap_mut().dup(w) }.unwrap();
}

/// 64 KiB の2つの文字列の `==`。`early` なら先頭のバイトで違う。
fn string_eq(early: bool) {
    let rounds = if early { 10_000_000 } else { 10_000 };
    let mut ctx = ctx();
    let a = string(&mut ctx, &"x".repeat(LONG));
    let other = if early {
        format!("y{}", "x".repeat(LONG - 1))
    } else {
        "x".repeat(LONG)
    };
    let b = string(&mut ctx, &other);
    for _ in 0..rounds {
        dup(&mut ctx, a);
        dup(&mut ctx, b);
        // SAFETY: a と b は生きた文字列で、渡す参照は直前の dup で作った。
        black_box(unsafe { ctx.string_eq(black_box(a), black_box(b)) }.unwrap());
    }
    decref(&mut ctx, a);
    decref(&mut ctx, b);
}

/// 末尾のバイトだけが違う 64 KiB の2つの文字列の `compare`。
fn compare() {
    let mut ctx = ctx();
    let a = string(&mut ctx, &format!("{}a", "x".repeat(LONG - 1)));
    let b = string(&mut ctx, &format!("{}b", "x".repeat(LONG - 1)));
    for _ in 0..10_000 {
        dup(&mut ctx, a);
        dup(&mut ctx, b);
        // SAFETY: string_eq と同じ。
        black_box(unsafe { ctx.string_compare(black_box(a), black_box(b)) }.unwrap());
    }
    decref(&mut ctx, a);
    decref(&mut ctx, b);
}

/// 共有した 1 KiB の左と 16 バイトの右の `++`。左は共有なので、毎回両辺を写して作り直す。
fn concat_copy() {
    let mut ctx = ctx();
    let left = string(&mut ctx, &"x".repeat(KIB));
    // 右は不死の定数にして、右を作る費用を計測に混ぜない。速いモードの `acquire_immortal` は何もしない
    // SAFETY: 不死の文字列を1つ確保するだけである。
    let right = unsafe { ctx.heap_mut() }.alloc_string_immortal("0123456789abcdef");
    for _ in 0..1_000_000 {
        dup(&mut ctx, left);
        // SAFETY: left は生きた文字列で、渡す参照は直前の dup で作った。right は生きた不死の物体である。
        let joined = unsafe {
            ctx.heap_mut().acquire_immortal(right).unwrap();
            ctx.string_append(black_box(left), black_box(right))
        }
        .unwrap();
        decref(&mut ctx, black_box(joined));
    }
    decref(&mut ctx, left);
}

fn show_int(value: i64) {
    let mut ctx = ctx();
    for _ in 0..10_000_000 {
        let shown = ctx.int_show(black_box(value));
        decref(&mut ctx, black_box(shown));
    }
}

fn show_string(text: &str) {
    let mut ctx = ctx();
    let s = string(&mut ctx, text);
    for _ in 0..100_000 {
        dup(&mut ctx, s);
        // SAFETY: s は生きた文字列で、渡す参照は直前の dup で作った。
        let shown = unsafe { ctx.string_show(black_box(s)) }.unwrap();
        decref(&mut ctx, black_box(shown));
    }
    decref(&mut ctx, s);
}

/// 23 バイトの行を `/dev/null` に書く。
fn println(mode: Buffering) {
    let null = std::fs::File::create("/dev/null").unwrap();
    let mut ctx = ctx_with(StdHost::new(
        Box::new(null),
        Box::new(io::sink()),
        PathBuf::new(),
        mode,
    ));
    let line = string(&mut ctx, "the quick brown fox jum");
    for _ in 0..1_000_000 {
        dup(&mut ctx, line);
        // SAFETY: line は生きた文字列で、渡す参照は直前の dup で作った。
        unsafe { ctx.write_string(Stream::Out, black_box(line)) }.unwrap();
    }
    ctx.flush().unwrap();
    decref(&mut ctx, line);
}

/// 1つのファイルだけを持つメモリの Host。中身は開くたびに写さず、`Arc` を分け合う。
struct OneFile {
    bytes: Arc<[u8]>,
    sized: bool,
}

impl Host for OneFile {
    fn write_line(&self, _: Stream, _: &str) -> io::Result<()> {
        Ok(())
    }
    fn flush(&self) -> io::Result<()> {
        Ok(())
    }
    fn open(&self, _: &str) -> io::Result<Opened> {
        Ok(Opened {
            reader: Box::new(Cursor::new(self.bytes.clone())),
            size: self.sized.then_some(self.bytes.len() as u64),
        })
    }
}

/// 1 MiB のファイルを開いて、全部読んで、閉じる。`sized` でなければ、Host は長さの見込みを返さない。
fn read_all(sized: bool) {
    let bytes: Arc<[u8]> = "abcdefg\n".repeat(MIB / 8).into_bytes().into();
    let mut ctx = ctx_with(OneFile { bytes, sized });
    // SAFETY: 不死の文字列を1つ確保するだけである。
    let path = unsafe { ctx.heap_mut() }.alloc_string_immortal("big.txt");
    for _ in 0..100 {
        // SAFETY: path は生きた不死の物体である。fs_read が返した組から File と文字列の参照を取り出し、組は
        // 両方を残して手放す。File は fs_close に、文字列は decref に渡し、その後どちらも読まない。
        unsafe {
            ctx.heap_mut().acquire_immortal(path).unwrap();
            let file = ctx.fs_open(path).unwrap();
            let pair = ctx.fs_read(file, false).unwrap();
            let file = ctx.heap().field(pair, 0).unwrap();
            let text = ctx.heap().field(pair, 1).unwrap();
            ctx.heap_mut().release(pair, TUPLE, &[true, true]).unwrap();
            black_box(text);
            ctx.heap_mut().decref(text).unwrap();
            ctx.fs_close(file).unwrap();
        }
    }
}
