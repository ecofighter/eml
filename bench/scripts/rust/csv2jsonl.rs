// csv2jsonl の Rust の実装。tests/ui/run/scripts/csv2jsonl/main.em と同じ標準出力、書き出すファイル、終了コードを返す基準である
// (docs/implementation/benchmarks.md の「スクリプトの計測」)。安全な std だけを使う。
// 比べるのは正しい入力の結果だけで、誤りの文言は eml の実装に合わせていない。
use std::borrow::Cow;
use std::io::Write;
fn fail(m: &str, code: i32) -> ! {
    eprintln!("csv2jsonl: {m}");
    std::process::exit(code)
}
fn json_string(out: &mut Vec<u8>, s: &[u8]) {
    out.push(b'"');
    let mut start = 0;
    for (i, &b) in s.iter().enumerate() {
        if b == b'"' || b == b'\\' || b < 32 {
            out.extend_from_slice(&s[start..i]);
            match b {
                b'"' => out.extend_from_slice(b"\\\""),
                b'\\' => out.extend_from_slice(b"\\\\"),
                8 => out.extend_from_slice(b"\\b"),
                12 => out.extend_from_slice(b"\\f"),
                b'\n' => out.extend_from_slice(b"\\n"),
                b'\r' => out.extend_from_slice(b"\\r"),
                b'\t' => out.extend_from_slice(b"\\t"),
                _ => {
                    write!(out, "\\u{:04x}", b).unwrap();
                }
            }
            start = i + 1;
        }
    }
    out.extend_from_slice(&s[start..]);
    out.push(b'"');
}
// 戻り値: 行ごとの欄。引用符の欄は `""` を `"` にした写し、それ以外は入力の範囲を借りる。
// 見出しの行を読んだ時点で `on_header` を呼ぶのは、後ろの行の誤りより先に列の誤りを出す eml の順に合わせるため
fn rows<'a>(t: &'a [u8], mut on_header: impl FnMut(&[Cow<'a, [u8]>])) -> Vec<Vec<Cow<'a, [u8]>>> {
    let n = t.len();
    let mut at = 0;
    let mut rows = vec![];
    while at < n {
        if t[at] == b'\n' {
            at += 1;
            continue;
        }
        if t[at] == b'\r' && at + 1 < n && t[at + 1] == b'\n' {
            at += 2;
            continue;
        }
        let mut fields: Vec<Cow<[u8]>> = vec![];
        loop {
            if at < n && t[at] == b'"' {
                let mut acc = vec![];
                at += 1;
                loop {
                    let close = match t[at..].iter().position(|&b| b == b'"') {
                        Some(p) => at + p,
                        None => fail("unclosed quote", 1),
                    };
                    acc.extend_from_slice(&t[at..close]);
                    if close + 1 < n && t[close + 1] == b'"' {
                        acc.push(b'"');
                        at = close + 2
                    } else {
                        at = close + 1;
                        break;
                    }
                }
                fields.push(Cow::Owned(acc));
                if at < n && t[at] == b',' {
                    at += 1;
                    continue;
                }
                if at >= n {
                } else if t[at] == b'\n' {
                    at += 1
                } else if t[at] == b'\r' && at + 1 < n && t[at + 1] == b'\n' {
                    at += 2
                } else {
                    fail("unexpected text after a closing quote", 1)
                }
                break;
            } else {
                let stop = t[at..]
                    .iter()
                    .position(|&b| b == b',' || b == b'\n')
                    .map_or(n, |p| at + p);
                let mut f = &t[at..stop];
                if f.last() == Some(&b'\r') {
                    f = &f[..f.len() - 1];
                }
                fields.push(Cow::Borrowed(f));
                if stop < n && t[stop] == b',' {
                    at = stop + 1;
                    continue;
                }
                at = if stop < n { stop + 1 } else { n };
                break;
            }
        }
        if rows.is_empty() {
            on_header(&fields);
        }
        rows.push(fields);
    }
    rows
}
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 4 || args[0] != "--columns" {
        fail("usage: csv2jsonl --columns NAME,NAME... INPUT OUTPUT", 2)
    }
    let columns: Vec<&str> = args[1].split(',').collect();
    let text = std::fs::read_to_string(&args[2]).unwrap_or_else(|e| fail(&e.to_string(), 1));
    let mut picked: Vec<usize> = vec![];
    let rows = rows(text.as_bytes(), |header| {
        picked = columns
            .iter()
            .map(|c| {
                header
                    .iter()
                    .position(|h| &h[..] == c.as_bytes())
                    .unwrap_or_else(|| fail(&format!("unknown column `{c}`"), 2))
            })
            .collect();
    });
    if rows.is_empty() {
        fail("line 1: missing header", 1)
    }
    let header = &rows[0];
    let mut out = Vec::with_capacity(text.len());
    for r in &rows[1..] {
        if r.len() != header.len() {
            fail(
                &format!("expected {} fields, found {}", header.len(), r.len()),
                1,
            )
        }
        out.push(b'{');
        for (k, (&i, c)) in picked.iter().zip(&columns).enumerate() {
            if k > 0 {
                out.push(b',');
            }
            json_string(&mut out, c.as_bytes());
            out.push(b':');
            json_string(&mut out, &r[i]);
        }
        out.extend_from_slice(b"}\n");
    }
    std::fs::write(&args[3], out).unwrap_or_else(|e| fail(&e.to_string(), 1));
}
