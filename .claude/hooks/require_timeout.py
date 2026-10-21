#!/usr/bin/env python3
# Bash で .em を timeout なしに走らせる呼び出しを止める PreToolUse フック。
# 末尾呼び出しの無限ループは max_depth にも max_bytes にも当たらないので
# (docs/spec/vm.md の「上限と割り込み」)、timeout がないと Claude Code が
# 120 秒後にバックグラウンドへ移し、プロセスとサブエージェントが残り続ける。
import json
import os
import re
import shlex
import sys

SEPARATORS = re.compile(r"&&|\|\||[;|&\n()`{}]")
PREFIX_WORDS = {"do", "then", "else", "!", "env", "command", "exec", "time", "nice"}
TIMEOUT_WORDS = {"timeout", "gtimeout"}
ASSIGNMENT = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*=")
# 終わることが分かっているサブコマンド
FINITE_EML_ARGS = {"check", "--help", "-h", "--version", "-V", "help"}


HEREDOC = re.compile(r"<<-?\s*(['\"]?)([A-Za-z_][A-Za-z0-9_]*)\1")


def strip_heredocs(command):
    # heredoc の中身はファイルに書くだけの文字列なので、コマンドとして読まない
    lines = command.split("\n")
    kept = []
    delimiters = []
    for line in lines:
        if delimiters:
            if line.strip() == delimiters[0]:
                delimiters.pop(0)
            continue
        kept.append(line)
        delimiters.extend(m.group(2) for m in HEREDOC.finditer(line))
    return "\n".join(kept)


def tokens(segment):
    try:
        return shlex.split(segment, comments=True)
    except ValueError:
        return segment.split()


def runs_em_unbounded(words):
    while words and (words[0] in PREFIX_WORDS or ASSIGNMENT.match(words[0])):
        words = words[1:]
    if not words or os.path.basename(words[0]) in TIMEOUT_WORDS:
        return False
    head = os.path.basename(words[0])
    if head == "eml":
        return len(words) > 1 and words[1] not in FINITE_EML_ARGS
    if head == "cargo" and len(words) > 1 and words[1] == "run":
        names_package = any(w in ("-p", "--package") or w.startswith("--package=") for w in words)
        if names_package and not any("eml_cli" in w for w in words):
            return False
        rest = words[words.index("--") + 1 :] if "--" in words else []
        return not (rest and rest[0] in FINITE_EML_ARGS)
    return False


def main():
    payload = json.load(sys.stdin)
    command = payload.get("tool_input", {}).get("command", "")
    offending = [
        s.strip()
        for s in SEPARATORS.split(strip_heredocs(command))
        if runs_em_unbounded(tokens(s))
    ]
    if not offending:
        return
    reason = (
        "Run .em programs under `timeout` (e.g. `timeout 20 cargo run -q -p eml_cli -- run f.em`): "
        "a non-allocating tail-recursive loop never hits the VM limits, and a Bash call past its "
        "timeout is moved to the background instead of being killed (CLAUDE.md の Rules). "
        "Offending: " + "; ".join(offending)
    )
    json.dump(
        {
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny",
                "permissionDecisionReason": reason,
            }
        },
        sys.stdout,
    )


main()
