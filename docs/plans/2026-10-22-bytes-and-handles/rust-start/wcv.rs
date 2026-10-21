// wc の Rust の版の書き方の違い: 2回走査か1回か、UTF-8 を確かめるか
use std::io::Read;
fn is_space(b: u8) -> bool { b == b' ' || (9..=13).contains(&b) }
fn two_pass(d: &[u8]) -> (u64, u64) {
    let lines = d.iter().filter(|&&b| b == b'\n').count() as u64;
    let mut words = 0u64; let mut in_word = false;
    for &b in d { let s = is_space(b); if !s && !in_word { words += 1; } in_word = !s; }
    (lines, words)
}
fn one_pass(d: &[u8]) -> (u64, u64) {
    let (mut lines, mut words, mut in_word) = (0u64, 0u64, false);
    for &b in d {
        if is_space(b) { if b == b'\n' { lines += 1 } if in_word { words += 1 } in_word = false } else { in_word = true }
    }
    (lines, if in_word { words + 1 } else { words })
}
fn main() {
    let mode = std::env::args().nth(1).unwrap();
    let mut v = Vec::new();
    let r = if mode.ends_with("utf8") { let mut s = String::new(); std::io::stdin().read_to_string(&mut s).unwrap(); v = s.into_bytes(); &v[..] } else { std::io::stdin().read_to_end(&mut v).unwrap(); &v[..] };
    let (l, w) = if mode.starts_with("two") { two_pass(r) } else { one_pass(r) };
    println!("{l} {w} {}", r.len());
}
