// logstat の Rust の実装。tests/ui/run/scripts/logstat/main.em と同じ標準出力と終了コードを返す基準である
// (docs/implementation/benchmarks.md の「スクリプトの計測」)。安全な std だけを使う。
// 順序つきの表は eml の `Map` に合わせて `BTreeMap` である。
use std::collections::BTreeMap;
use std::io::{BufRead, Read, Write};

fn to_int(s: &str) -> Option<i64> {
    s.parse::<i64>().ok()
}

fn usage() -> ! {
    eprintln!("usage: logstat [-n N] [--whole]");
    std::process::exit(2)
}

#[derive(Default)]
struct Stats {
    total: u64,
    malformed: u64,
    count: i64,
    sum: i64,
    max: i64,
    statuses: BTreeMap<i64, u64>,
    paths: BTreeMap<String, u64>,
}

impl Stats {
    fn add(&mut self, line: &str) {
        self.total += 1;
        let fields: Vec<&str> = line.split(' ').collect();
        let parsed = if fields.len() == 5 {
            to_int(fields[3]).zip(to_int(fields[4]))
        } else {
            None
        };
        let Some((status, duration)) = parsed else {
            if self.malformed == 0 {
                eprintln!("logstat: line {}: malformed", self.total);
            }
            self.malformed += 1;
            return;
        };
        *self.statuses.entry(status).or_insert(0) += 1;
        match self.paths.get_mut(fields[2]) {
            Some(n) => *n += 1,
            None => {
                self.paths.insert(fields[2].to_string(), 1);
            }
        }
        if self.count == 0 || duration > self.max {
            self.max = duration;
        }
        self.count += 1;
        self.sum += duration;
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mut top, mut whole) = (10usize, false);
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--whole" {
            whole = true;
            i += 1;
        } else if args[i] == "-n" && i + 1 < args.len() {
            match to_int(&args[i + 1]) {
                Some(k) if k >= 0 => {
                    top = k as usize;
                    i += 2;
                }
                _ => usage(),
            }
        } else {
            usage()
        }
    }
    let mut stats = Stats::default();
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    if whole {
        let mut text = String::new();
        input.read_to_string(&mut text).unwrap();
        for l in text.lines() {
            stats.add(l);
        }
    } else {
        let mut line = String::new();
        loop {
            line.clear();
            if input.read_line(&mut line).unwrap() == 0 {
                break;
            }
            let mut l = line.as_str();
            if let Some(s) = l.strip_suffix('\n') {
                l = s.strip_suffix('\r').unwrap_or(s);
            }
            stats.add(l);
        }
    }
    let out = std::io::stdout();
    let mut out = std::io::BufWriter::new(out.lock());
    writeln!(out, "lines {}", stats.total).unwrap();
    writeln!(out, "malformed {}", stats.malformed).unwrap();
    for (s, n) in &stats.statuses {
        writeln!(out, "status {s} {n}").unwrap();
    }
    // 安定な並べ替えなので、同じ回数は表の順 (パスの昇順) のままである
    let mut by: Vec<(&String, &u64)> = stats.paths.iter().collect();
    by.sort_by(|a, b| b.1.cmp(a.1));
    for (p, n) in by.into_iter().take(top) {
        writeln!(out, "path {p} {n}").unwrap();
    }
    let avg = if stats.count == 0 {
        0
    } else {
        stats.sum / stats.count
    };
    writeln!(out, "duration avg {avg} max {}", stats.max).unwrap();
}
