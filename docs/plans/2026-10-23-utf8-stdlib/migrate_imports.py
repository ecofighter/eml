#!/usr/bin/env python3
"""標準ライブラリのモジュールを import なしで修飾している .em と .repl に import を足す。

usage: migrate_imports.py [--dry-run] ROOT...   (ROOT は tests/ui、bench、crates/eml_cli/tests/repl などのディレクトリ)
"""
import pathlib
import re
import sys

STD = "Fs List String Env Stdin Stdout Stderr Proc Yield Fail State Iter Float Char Byte Bytes Array MutArray".split()
QUALIFIER = re.compile(r"(?<![\w.])([A-Z][A-Za-z0-9_]*)\.(?=[A-Za-z_(])")
IMPORT = re.compile(r"^import\s+((?:[A-Z]\w*\.)*)([A-Z]\w*)(?:\s+as\s+([A-Z]\w*))?", re.M)


def code_only(text):
    """コメントと文字列の本文を除いた、名前を引く部分だけを返す。補間の穴 `\\{...}` の中は残す。"""
    out, i, n = [], 0, len(text)
    while i < n:
        c = text[i]
        if text.startswith("--", i):
            j = text.find("\n", i)
            i = n if j < 0 else j
        elif text.startswith('"""', i):
            i = skip_string(text, i + 3, '"""', out)
        elif c == "r" and i + 1 < n and text[i + 1] in '"#' and (i == 0 or not (text[i - 1].isalnum() or text[i - 1] == "_")):
            j = i + 1
            hashes = 0
            while j < n and text[j] == "#":
                hashes += 1
                j += 1
            if j < n and text[j] == '"':
                end = text.find('"' + "#" * hashes, j + 1)
                i = n if end < 0 else end + 1 + hashes
            else:
                out.append(c)
                i += 1
        elif c == '"':
            i = skip_string(text, i + 1, '"', out)
        elif c == "'" and i + 2 < n and (text[i + 2] == "'" or (text[i + 1] == "\\" and text.find("'", i + 2) > 0)):
            i = text.find("'", i + 1 if text[i + 1] != "\\" else i + 2) + 1
        else:
            out.append(c)
            i += 1
    return "".join(out)


def skip_string(text, i, close, out):
    n = len(text)
    while i < n:
        if text.startswith(close, i):
            return i + len(close)
        if text.startswith("\\{", i):
            depth, j = 1, i + 2
            while j < n and depth:
                depth += {"{": 1, "}": -1}.get(text[j], 0)
                j += 1
            out.append(" " + text[i + 2 : j - 1] + " ")
            i = j
        elif text[i] == "\\":
            i += 2
        else:
            i += 1
    return n


def qualified_std(text):
    """import なしで修飾子として使っている標準ライブラリのモジュールの名前の集合。`Std.X.` の形は数えない。"""
    code = code_only(text)
    imported = {alias or last for _, last, alias in IMPORT.findall(text)}
    return {q for q in QUALIFIER.findall(code) if q in STD and q not in imported}


def siblings(path):
    return {p.stem.lower(): p.name for p in path.parent.glob("*.em")}


def import_line(module, path):
    # 同じディレクトリに大文字小文字だけ違う `X.em` があれば `import X` は E1026 になるので、正式な名前で書く
    return f"import Std.{module}" if module.lower() in siblings(path) else f"import {module}"


def rewrite_em(path, text):
    missing = sorted(qualified_std(text))
    if not missing:
        return None
    lines = text.split("\n")
    i = 0
    if lines and lines[0].startswith("#!"):
        i = 1
    while i < len(lines) and (lines[i].startswith("--") or not lines[i].strip()):
        i += 1
    header_end = i
    last_import = None
    while i < len(lines) and (lines[i].startswith("import ") or not lines[i].strip() or lines[i].startswith("--")):
        if lines[i].startswith("import "):
            last_import = i
        i += 1
    new = [import_line(m, path) for m in missing]
    if last_import is not None:
        lines[last_import + 1 : last_import + 1] = new
    else:
        at = header_end
        while at > 0 and not lines[at - 1].strip():
            at -= 1
        lines[at:at] = new + [""]
    return "\n".join(lines)


def rewrite_repl(path, text):
    lines = text.split("\n")
    inputs = "\n".join(l[2:] for l in lines if l.startswith("> ") or l.startswith(". "))
    missing = sorted(qualified_std(inputs))
    if not missing:
        return None
    at = 0
    while at < len(lines) and (lines[at].startswith("#") or not lines[at].strip() or lines[at].startswith("> :")):
        at += 1
    lines[at:at] = [f"> import {m}" for m in missing]
    return "\n".join(lines)


def main(argv):
    dry = "--dry-run" in argv
    roots = [a for a in argv if not a.startswith("--")]
    for root in roots:
        for path in sorted(pathlib.Path(root).rglob("*")):
            if path.suffix not in (".em", ".repl"):
                continue
            text = path.read_text()
            new = (rewrite_em if path.suffix == ".em" else rewrite_repl)(path, text)
            if new is None:
                continue
            print(path, sorted(qualified_std(text if path.suffix == ".em" else "\n".join(l[2:] for l in text.split("\n") if l[:2] in ("> ", ". ")))))
            if not dry:
                path.write_text(new)


if __name__ == "__main__":
    main(sys.argv[1:])
