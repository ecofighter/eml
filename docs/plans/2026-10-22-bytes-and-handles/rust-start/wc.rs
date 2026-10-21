use std::io::{Read, Write};
fn is_space(b: u8) -> bool { b == b' ' || (9..=13).contains(&b) }
fn counts(d: &[u8]) -> [u64; 3] {
    let lines = d.iter().filter(|&&b| b == b'\n').count() as u64;
    let mut words = 0u64; let mut in_word = false;
    for &b in d { let s = is_space(b); if !s && !in_word { words += 1; } in_word = !s; }
    [lines, words, d.len() as u64]
}
fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut chosen = vec![];
    while let Some(a) = args.first() { match a.as_str() { "-l" => chosen.push(0), "-w" => chosen.push(1), "-c" => chosen.push(2), _ => break } args.remove(0); }
    if let Some(a) = args.first() { if a.starts_with('-') && a != "-" { eprintln!("usage: wc [-l] [-w] [-c] [FILE...]"); std::process::exit(2) } }
    chosen.sort(); chosen.dedup(); if chosen.is_empty() { chosen = vec![0, 1, 2]; }
    let names: Vec<Option<String>> = if args.is_empty() { vec![None] } else { args.into_iter().map(Some).collect() };
    let mut total = [0u64; 3]; let mut failed = false;
    let out = std::io::stdout(); let mut out = out.lock();
    for name in &names {
        let data = match name.as_deref() {
            None | Some("-") => { let mut v = vec![]; std::io::stdin().read_to_end(&mut v).unwrap(); v }
            Some(n) => match std::fs::read(n) { Ok(v) => v, Err(e) => { eprintln!("wc: {n}: {e}"); failed = true; continue } },
        };
        let c = counts(&data);
        for i in 0..3 { total[i] += c[i]; }
        let mut f: Vec<String> = chosen.iter().map(|&i| c[i].to_string()).collect();
        if let Some(n) = name { f.push(n.clone()); }
        writeln!(out, "{}", f.join(" ")).unwrap();
    }
    if names.len() >= 2 { let mut f: Vec<String> = chosen.iter().map(|&i| total[i].to_string()).collect(); f.push("total".into()); writeln!(out, "{}", f.join(" ")).unwrap(); }
    std::process::exit(if failed { 1 } else { 0 })
}
