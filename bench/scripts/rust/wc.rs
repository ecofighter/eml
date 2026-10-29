// wc の Rust の実装。tests/ui/run/scripts/wc/main.em と同じ標準出力と終了コードを返す基準である
// (docs/implementation/benchmarks.md の「スクリプトの計測」)。安全な std だけを使う。
// eml の実装に合わせ、入力を UTF-8 の文字列として1回だけ走査する。
use std::io::{Read, Write};

// 語は ASCII の空白 (空白と 0x09 から 0x0D) で区切る
fn is_space(b: u8) -> bool {
    b == b' ' || (9..=13).contains(&b)
}

fn counts(text: &str) -> [u64; 3] {
    let (mut lines, mut words, mut in_word) = (0u64, 0u64, false);
    for &b in text.as_bytes() {
        if is_space(b) {
            if b == b'\n' {
                lines += 1;
            }
            in_word = false;
        } else {
            if !in_word {
                words += 1;
            }
            in_word = true;
        }
    }
    [lines, words, text.len() as u64]
}

fn read(name: Option<&str>) -> Result<String, String> {
    match name {
        None | Some("-") => {
            let mut text = String::new();
            std::io::stdin()
                .read_to_string(&mut text)
                .map_err(|e| e.to_string())?;
            Ok(text)
        }
        Some(path) => std::fs::read_to_string(path).map_err(|e| e.to_string()),
    }
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut chosen = vec![];
    while let Some(a) = args.first() {
        match a.as_str() {
            "-l" => chosen.push(0),
            "-w" => chosen.push(1),
            "-c" => chosen.push(2),
            _ => break,
        }
        args.remove(0);
    }
    if args.first().is_some_and(|a| a.starts_with('-') && a != "-") {
        eprintln!("usage: wc [-l] [-w] [-c] [FILE...]");
        std::process::exit(2)
    }
    chosen.sort();
    chosen.dedup();
    if chosen.is_empty() {
        chosen = vec![0, 1, 2];
    }
    let names: Vec<Option<String>> = if args.is_empty() {
        vec![None]
    } else {
        args.into_iter().map(Some).collect()
    };
    let mut total = [0u64; 3];
    let mut failed = false;
    let out = std::io::stdout();
    let mut out = out.lock();
    for name in &names {
        let text = match read(name.as_deref()) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("wc: {}: {e}", name.as_deref().unwrap_or("-"));
                failed = true;
                continue;
            }
        };
        let found = counts(&text);
        for i in 0..3 {
            total[i] += found[i];
        }
        let mut fields: Vec<String> = chosen.iter().map(|&i| found[i].to_string()).collect();
        if let Some(n) = name {
            fields.push(n.clone());
        }
        writeln!(out, "{}", fields.join(" ")).unwrap();
    }
    if names.len() >= 2 {
        let mut fields: Vec<String> = chosen.iter().map(|&i| total[i].to_string()).collect();
        fields.push("total".into());
        writeln!(out, "{}", fields.join(" ")).unwrap();
    }
    std::process::exit(if failed { 1 } else { 0 })
}
