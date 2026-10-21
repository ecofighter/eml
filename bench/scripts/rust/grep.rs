// grep の Rust の版。tests/ui/run/scripts/grep/main.em と同じ標準出力と終了コードを返す基準である
// (docs/implementation/benchmarks.md の「スクリプトの計測」)。安全な std だけを使う。
// 標準入力は1行ずつ、ファイルは全体を読んでから `\n` で分け、各行の末尾の `\r` を1つ除く。
use std::io::{BufRead, Write};

struct Options {
    invert: bool,
    number: bool,
    count: bool,
}

// 一致した行の数を返す。出力は呼び出し側の buffer に書く
fn scan(
    lines: impl Iterator<Item = Result<String, String>>,
    pattern: &str,
    prefix: &str,
    o: &Options,
    out: &mut impl Write,
) -> Result<u64, String> {
    let mut count = 0u64;
    for (i, line) in lines.enumerate() {
        let line = line?;
        if line.contains(pattern) != o.invert {
            count += 1;
            if !o.count {
                if o.number {
                    write!(out, "{prefix}{}:", i + 1).unwrap();
                } else {
                    write!(out, "{prefix}").unwrap();
                }
                out.write_all(line.as_bytes()).unwrap();
                out.write_all(b"\n").unwrap();
            }
        }
    }
    if o.count {
        writeln!(out, "{prefix}{count}").unwrap();
    }
    Ok(count)
}

fn stdin_lines() -> impl Iterator<Item = Result<String, String>> {
    let mut input = std::io::stdin().lock();
    std::iter::from_fn(move || {
        let mut line = String::new();
        match input.read_line(&mut line) {
            Ok(0) => None,
            Err(e) => Some(Err(e.to_string())),
            Ok(_) => {
                if line.ends_with('\n') {
                    line.pop();
                    if line.ends_with('\r') {
                        line.pop();
                    }
                }
                Some(Ok(line))
            }
        }
    })
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut o = Options {
        invert: false,
        number: false,
        count: false,
    };
    while let Some(a) = args.first() {
        match a.as_str() {
            "-v" => o.invert = true,
            "-n" => o.number = true,
            "-c" => o.count = true,
            _ => break,
        }
        args.remove(0);
    }
    if args.is_empty() || (args[0].starts_with('-') && args[0] != "-") {
        eprintln!("usage: grep [-v] [-n] [-c] PATTERN [FILE...]");
        std::process::exit(2)
    }
    let pattern = args.remove(0);
    let names = if args.is_empty() {
        vec!["-".to_string()]
    } else {
        args
    };
    let prefixed = names.len() >= 2;
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::with_capacity(64 * 1024, stdout.lock());
    let (mut failed, mut matched) = (false, false);
    for name in &names {
        let prefix = if prefixed {
            format!("{name}:")
        } else {
            String::new()
        };
        let found = if name == "-" {
            scan(stdin_lines(), &pattern, &prefix, &o, &mut out)
        } else {
            std::fs::read_to_string(name)
                .map_err(|e| e.to_string())
                .and_then(|text| {
                    let lines = text.lines().map(|l| Ok(l.to_string()));
                    scan(lines, &pattern, &prefix, &o, &mut out)
                })
        };
        match found {
            Ok(count) => matched |= count > 0,
            Err(e) => {
                eprintln!("grep: {name}: {e}");
                failed = true;
            }
        }
    }
    out.flush().unwrap();
    std::process::exit(if failed {
        2
    } else if matched {
        0
    } else {
        1
    })
}
