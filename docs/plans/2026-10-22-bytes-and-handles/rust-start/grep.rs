use std::io::{BufRead, Write};
fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let (mut v, mut n, mut c) = (false, false, false);
    while let Some(a) = args.first() { match a.as_str() { "-v" => v = true, "-n" => n = true, "-c" => c = true, _ => break } args.remove(0); }
    if args.is_empty() || (args[0].starts_with('-') && args[0] != "-") { eprintln!("usage: grep [-v] [-n] [-c] PATTERN [FILE...]"); std::process::exit(2) }
    let pattern = args.remove(0);
    if !args.is_empty() { eprintln!("files not supported in this baseline"); std::process::exit(2) }
    let stdin = std::io::stdin(); let mut input = stdin.lock();
    let out = std::io::stdout(); let mut out = std::io::BufWriter::with_capacity(64 * 1024, out.lock());
    let mut line = String::new(); let mut number = 0u64; let mut count = 0u64;
    loop {
        line.clear();
        if input.read_line(&mut line).unwrap() == 0 { break }
        number += 1;
        let mut l = line.as_str();
        if let Some(s) = l.strip_suffix('\n') { l = s.strip_suffix('\r').unwrap_or(s); }
        if l.contains(pattern.as_str()) != v {
            count += 1;
            if !c { if n { write!(out, "{number}:").unwrap(); } out.write_all(l.as_bytes()).unwrap(); out.write_all(b"\n").unwrap(); }
        }
    }
    if c { writeln!(out, "{count}").unwrap(); }
    out.flush().unwrap();
    std::process::exit(if count > 0 { 0 } else { 1 })
}
