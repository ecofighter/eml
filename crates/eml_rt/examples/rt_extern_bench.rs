//! `eml_rt` の extern の速いモードの計測 (docs/implementation/benchmarks.md)。`bench/rt.sh` が release ビルドで
//! 走らせ、命令の数を測る。`rt_bench` と別のプログラムにするのは、今ある14の計測のコード生成単位の分け方を変えない
//! ためである (docs/implementation/benchmarks.md の「`eml_rt` のインライン化」)。

use std::hint::black_box;
use std::io::{self, BufRead, Cursor, Read};
use std::path::PathBuf;
use std::sync::Arc;

use eml_rt::{
    Buffering, Control, Ctx, Fast, Form, Handle, Host, HostRef, Limits, Opened, ReadAmount,
    StdHost, Stream, Word, Write,
};

const LONG: usize = 64 * 1024;
const KIB: usize = 1024;
const MIB: usize = 1024 * 1024;

fn main() {
    let case = std::env::args().nth(1).expect(
        "a case: string-eq, string-eq-early, compare, concat-copy, show-int, show-int-long, \
             show-string, show-string-dense, println-block, println-line, read-all, read-all-unsized, \
             find-byte-1mib, find-byte-1mib-position, find-bytes-1mib, find-bytes-1mib-windows, \
             find-byte, find-byte-swar, find-sub, find-sub-contains, slice-short, slice-short-box, \
             read-all-bytes, read-all-bytes-std, read-line-bytes-64mib, read-line-bytes-64mib-std, \
             stdout-write-line-10m",
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
        "find-byte-1mib" => find_byte(false),
        "find-byte-1mib-position" => find_byte(true),
        "find-bytes-1mib" => find_bytes(false),
        "find-bytes-1mib-windows" => find_bytes(true),
        "find-byte" => find_newline(false),
        "find-byte-swar" => find_newline(true),
        "find-sub" => find_sub(false),
        "find-sub-contains" => find_sub(true),
        "slice-short" => slice_short(false),
        "slice-short-box" => slice_short(true),
        "read-all-bytes" => read_all_bytes(false),
        "read-all-bytes-std" => read_all_bytes(true),
        "read-line-bytes-64mib" => read_line_bytes(false),
        "read-line-bytes-64mib-std" => read_line_bytes(true),
        "stdout-write-line-10m" => stdout_write_line(),
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

/// 参照を1つ足す。引数を消費する extern に、同じ文字列を何度も渡すために使う。借りる extern には要らない。
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
        // SAFETY: a と b は生きた文字列で、呼ぶ側が参照を持つ。string_eq は借りるだけで手放さない。
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
        // SAFETY: s は生きた文字列で、呼ぶ側が参照を持つ。string_show は借りるだけで手放さない。
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

/// 23 バイトの行を `Stdout.write_line` で 10,000,000 回書く。Host は `io::sink` へまとめて書く `StdHost` なので、
/// 1回の費用 (extern、`dyn Host` の呼び出し、競合しない lock、buffer への写し) を測る。
fn stdout_write_line() {
    let mut ctx = ctx();
    let line = string(&mut ctx, "the quick brown fox jum");
    let mut out = ctx.handle_open(Handle::Stdout).unwrap();
    for _ in 0..10_000_000 {
        // SAFETY: out は生きた標準出力のハンドルで、handle_write は受け取ったハンドルを返す。line は借りるだけで
        // 手放さない。
        out = unsafe { ctx.handle_write(out, Stream::Out, black_box(line), Write::Line) }.unwrap();
    }
    // SAFETY: out は生きた標準出力のハンドルで、この後読まない。
    unsafe { ctx.handle_close(out, Handle::Stdout) }.unwrap();
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
    fn write(&self, _: Stream, _: &[u8]) -> io::Result<()> {
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
        // SAFETY: path は生きた不死の物体である。handle_read は File を借りて文字列を返す。File は handle_close に、
        // 文字列は decref に渡し、その後どちらも読まない。
        unsafe {
            ctx.heap_mut().acquire_immortal(path).unwrap();
            let file = ctx.fs_open(path).unwrap();
            let text = ctx
                .handle_read(file, Handle::File, ReadAmount::All, Form::Text)
                .unwrap();
            black_box(text);
            ctx.heap_mut().decref(text).unwrap();
            ctx.handle_close(file, Handle::File).unwrap();
        }
    }
}

/// 52 バイトの行を並べた 1 MiB のバイト列。末尾の `last` で終わる。行の中の `0` は、`find_bytes` の needle の先頭の
/// バイトとして、行ごとに1回合う。
fn haystack(last: &[u8]) -> Vec<u8> {
    let line = b"abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNO\n";
    let mut hay: Vec<u8> = line
        .iter()
        .copied()
        .cycle()
        .take(MIB - last.len())
        .collect();
    hay.extend_from_slice(last);
    hay
}

/// 1 MiB のバイト列の末尾の1バイトを `Bytes.find_byte_from` で探す。`std` なら、同じ探索を
/// `<[u8]>::iter().position` で行う (比べる基準)。
fn find_byte(std: bool) {
    let hay = haystack(b"#");
    let mut ctx = ctx();
    // SAFETY: バイト列を1つ確保するだけで、ほかの物体には触れない。
    let w = unsafe { ctx.heap_mut() }.alloc_bytes(&hay);
    let target = black_box(b'#');
    for _ in 0..1_000 {
        let at = if std {
            black_box(&hay[..])
                .iter()
                .position(|&x| x == target)
                .map_or(-1, |i| i as i64)
        } else {
            // SAFETY: w は生きたバイト列で、呼ぶ側が参照を持つ。bytes_find_byte は借りるだけで手放さない。
            unsafe { ctx.bytes_find_byte(target as i64, 0, black_box(w)) }.unwrap()
        };
        assert_eq!(black_box(at), MIB as i64 - 1);
    }
    decref(&mut ctx, w);
}

/// 1 MiB のバイト列の末尾の 16 バイトを `Bytes.find_from` で探す。needle の先頭の `0` は行ごとに合い、そのたびに
/// 15 バイト目まで比べて外れる。`std` なら、同じ探索を `<[u8]>::windows().position` で行う (比べる基準)。
fn find_bytes(std: bool) {
    let needle = b"0123456789ABCDE#";
    let hay = haystack(needle);
    let mut ctx = ctx();
    // SAFETY: バイト列を2つ確保するだけで、ほかの物体には触れない。
    let w = unsafe { ctx.heap_mut() }.alloc_bytes(&hay);
    let n = unsafe { ctx.heap_mut() }.alloc_bytes(needle);
    for _ in 0..100 {
        let at = if std {
            let needle = black_box(&needle[..]);
            black_box(&hay[..])
                .windows(needle.len())
                .position(|x| x == needle)
                .map_or(-1, |i| i as i64)
        } else {
            // SAFETY: n と w は生きたバイト列で、呼ぶ側が参照を持つ。bytes_find_from は借りるだけで手放さない。
            unsafe { ctx.bytes_find_from(black_box(n), 0, black_box(w)) }.unwrap()
        };
        assert_eq!(black_box(at), (MIB - needle.len()) as i64);
    }
    decref(&mut ctx, n);
    decref(&mut ctx, w);
}

/// 行ごとの探索の計測が使う 52 バイトの行。最後の1バイトが `\n` である。
const LINE: &[u8; 52] = b"abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNO\n";
const LINE_ROUNDS: usize = 10_000_000;

/// `b` が最初に現れる位置を、8 バイトずつの語の演算 (SWAR) で探す。`find_byte` (docs/spec/object-model.md の T13)
/// と同じ作りを Rust だけで書いた、比べる基準である。
fn swar_position(hay: &[u8], b: u8) -> Option<usize> {
    const LOW: u64 = 0x0101_0101_0101_0101;
    const HIGH: u64 = 0x8080_8080_8080_8080;
    let pattern = LOW * b as u64;
    let (chunks, rest) = hay.as_chunks::<8>();
    let mut at = 0;
    for chunk in chunks {
        let word = u64::from_le_bytes(*chunk) ^ pattern;
        let zero = word.wrapping_sub(LOW) & !word & HIGH;
        if zero != 0 {
            return Some(at + (zero.trailing_zeros() / 8) as usize);
        }
        at += 8;
    }
    rest.iter().position(|&x| x == b).map(|i| at + i)
}

/// 52 バイトの行で次の `\n` を 1000 万回探す。`swar` なら Rust の SWAR のループで探す。
fn find_newline(swar: bool) {
    let mut ctx = ctx();
    // SAFETY: バイト列を1つ確保するだけで、ほかの物体には触れない。
    let w = unsafe { ctx.heap_mut() }.alloc_bytes(LINE);
    let target = black_box(b'\n');
    for _ in 0..LINE_ROUNDS {
        let at = if swar {
            black_box(swar_position(black_box(&LINE[..]), target)).map_or(-1, |i| i as i64)
        } else {
            // SAFETY: w は生きたバイト列で、呼ぶ側が参照を持つ。bytes_find_byte は借りるだけで手放さない。
            unsafe { ctx.bytes_find_byte(target as i64, 0, black_box(w)) }.unwrap()
        };
        assert_eq!(black_box(at), 51);
    }
    decref(&mut ctx, w);
}

/// 16 バイトの needle を 52 バイトの行で 1000 万回探す。`contains` なら `str::contains` で探す。
fn find_sub(contains: bool) {
    let needle = &LINE[36..];
    let mut ctx = ctx();
    // SAFETY: バイト列を2つ確保するだけで、ほかの物体には触れない。
    let w = unsafe { ctx.heap_mut() }.alloc_bytes(LINE);
    let n = unsafe { ctx.heap_mut() }.alloc_bytes(needle);
    let line_str = std::str::from_utf8(LINE).unwrap();
    let needle_str = std::str::from_utf8(needle).unwrap();
    for _ in 0..LINE_ROUNDS {
        if contains {
            assert!(black_box(line_str).contains(black_box(needle_str)));
        } else {
            // SAFETY: n と w は生きたバイト列で、呼ぶ側が参照を持つ。bytes_find_from は借りるだけで手放さない。
            let at = unsafe { ctx.bytes_find_from(black_box(n), 0, black_box(w)) }.unwrap();
            assert_eq!(black_box(at), 36);
        }
    }
    decref(&mut ctx, n);
    decref(&mut ctx, w);
}

/// 先頭の 50 バイトの切り出しと手放しを 1000 万回行う。`boxed` なら `Box<[u8]>::from(&s[..50])` と drop。
fn slice_short(boxed: bool) {
    let mut ctx = ctx();
    // SAFETY: バイト列を1つ確保するだけで、ほかの物体には触れない。
    let w = unsafe { ctx.heap_mut() }.alloc_bytes(LINE);
    for _ in 0..LINE_ROUNDS {
        if boxed {
            let copy: Box<[u8]> = Box::from(&black_box(&LINE[..])[..50]);
            drop(black_box(copy));
        } else {
            // SAFETY: w は生きたバイト列で、呼ぶ側が参照を持つ。bytes_slice は借りるだけで手放さない。
            let part = unsafe { ctx.bytes_slice(0, 50, black_box(w)) }.unwrap();
            decref(&mut ctx, black_box(part));
        }
    }
    decref(&mut ctx, w);
}

/// 64 MiB のメモリの Host のファイルを、`handle_read` の `Form::Bytes` で全部読む。`std` なら `&[u8]` からの
/// `read_to_end` である。
fn read_all_bytes(std: bool) {
    const ROUNDS: usize = 8;
    let bytes: Arc<[u8]> = "abcdefg\n".repeat(64 * MIB / 8).into_bytes().into();
    if std {
        for _ in 0..ROUNDS {
            let mut out = Vec::new();
            black_box(&bytes[..]).read_to_end(&mut out).unwrap();
            black_box(out);
        }
        return;
    }
    let mut ctx = ctx_with(OneFile { bytes, sized: true });
    // SAFETY: 不死の文字列を1つ確保するだけである。
    let path = unsafe { ctx.heap_mut() }.alloc_string_immortal("big.txt");
    for _ in 0..ROUNDS {
        // SAFETY: read_all と同じ。
        unsafe {
            ctx.heap_mut().acquire_immortal(path).unwrap();
            let file = ctx.fs_open(path).unwrap();
            let read = ctx
                .handle_read(file, Handle::File, ReadAmount::All, Form::Bytes)
                .unwrap();
            black_box(read);
            ctx.heap_mut().decref(read).unwrap();
            ctx.handle_close(file, Handle::File).unwrap();
        }
    }
}

/// 56 バイトの行を並べた 64 MiB のメモリの Host のファイルを、`handle_read` の `Form::Bytes` で1行ずつ読み、行を
/// 手放す。`std` なら、64 KiB の `BufReader` の `read_until` で、行ごとに新しい `Vec` に読んで落とす (行ごとに確保
/// するのは eml の側と同じである)。
fn read_line_bytes(std: bool) {
    let line = "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRS\n";
    let bytes: Arc<[u8]> = line.repeat(64 * MIB / line.len()).into_bytes().into();
    if std {
        let mut reader = io::BufReader::with_capacity(LONG, Cursor::new(bytes));
        loop {
            let mut out = Vec::new();
            if reader.read_until(b'\n', &mut out).unwrap() == 0 {
                break;
            }
            black_box(out);
        }
        return;
    }
    let mut ctx = ctx_with(OneFile { bytes, sized: true });
    // SAFETY: 不死の文字列を1つ確保するだけである。
    let path = unsafe { ctx.heap_mut() }.alloc_string_immortal("big.txt");
    // SAFETY: read_all と同じ。行は decref に渡し、その後読まない。
    unsafe {
        ctx.heap_mut().acquire_immortal(path).unwrap();
        let file = ctx.fs_open(path).unwrap();
        loop {
            let read = ctx
                .handle_read(file, Handle::File, ReadAmount::Line, Form::Bytes)
                .unwrap();
            let len = ctx.heap().bytes(read).unwrap().len();
            ctx.heap_mut().decref(black_box(read)).unwrap();
            if len == 0 {
                break;
            }
        }
        ctx.handle_close(file, Handle::File).unwrap();
    }
}
