use std::collections::BTreeMap;
use std::io::{BufRead, Read, Write};
fn to_int(s: &str) -> Option<i64> { s.parse::<i64>().ok() }
fn usage() -> ! { eprintln!("usage: logstat [-n N] [--whole]"); std::process::exit(2) }
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mut top, mut whole) = (10usize, false);
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--whole" { whole = true; i += 1 }
        else if args[i] == "-n" && i + 1 < args.len() { match to_int(&args[i + 1]) { Some(k) if k >= 0 => { top = k as usize; i += 2 } _ => usage() } }
        else { usage() }
    }
    let (mut total, mut malformed, mut count, mut sum, mut max) = (0u64, 0u64, 0i64, 0i64, 0i64);
    let mut statuses: BTreeMap<i64, u64> = BTreeMap::new();
    let mut paths: BTreeMap<String, u64> = BTreeMap::new();
    let mut handle = |line: &str| {
        total += 1;
        let fields: Vec<&str> = line.split(' ').collect();
        let parsed = if fields.len() == 5 { to_int(fields[3]).zip(to_int(fields[4])) } else { None };
        match parsed {
            None => { if malformed == 0 { eprintln!("logstat: line {total}: malformed"); } malformed += 1 }
            Some((s, d)) => {
                *statuses.entry(s).or_insert(0) += 1;
                match paths.get_mut(fields[2]) { Some(c) => *c += 1, None => { paths.insert(fields[2].to_string(), 1); } }
                if count == 0 || d > max { max = d }
                count += 1; sum += d;
            }
        }
    };
    let stdin = std::io::stdin(); let mut input = stdin.lock();
    if whole {
        let mut text = String::new(); input.read_to_string(&mut text).unwrap();
        for l in text.lines() { handle(l) }
    } else {
        let mut line = String::new();
        loop {
            line.clear();
            if input.read_line(&mut line).unwrap() == 0 { break }
            let mut l = line.as_str();
            if let Some(s) = l.strip_suffix('\n') { l = s.strip_suffix('\r').unwrap_or(s); }
            handle(l)
        }
    }
    let out = std::io::stdout(); let mut out = std::io::BufWriter::new(out.lock());
    writeln!(out, "lines {total}").unwrap();
    writeln!(out, "malformed {malformed}").unwrap();
    for (s, n) in &statuses { writeln!(out, "status {s} {n}").unwrap(); }
    let mut by: Vec<(&String, &u64)> = paths.iter().collect();
    by.sort_by(|a, b| b.1.cmp(a.1));
    for (p, n) in by.into_iter().take(top) { writeln!(out, "path {p} {n}").unwrap(); }
    let avg = if count == 0 { 0 } else { sum / count };
    writeln!(out, "duration avg {avg} max {max}").unwrap();
}
