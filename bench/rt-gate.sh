#!/usr/bin/env bash
# eml_rt を変えたコミットの門 (docs/implementation/benchmarks.md の「命令の数の測り方」)。1つの作業ツリーで BASE と今の
# HEAD を交互に checkout して bench/rt.sh を流し、行ごとに命令の数の差 (%) の中央値と、門の行の命令の数の差の和を出す。
# 揺れる行 (empty、println-*、read-*、stdout-*) は見ない。ほかの行の中央値がどれも +0.1 % 以下なら 0 で終わる。
# `--allow '行 行…' --allow-bound 上限` を付けると、挙げた行は上限 (%) まで許し、その代わり和が負であることを求める。
# 使い方: bench/rt-gate.sh [--allow '行 行…' --allow-bound 上限] BASE [組の数 (既定 5)]
set -euo pipefail

main() {
  local allow="" bound="0.1"
  while [[ "${1:-}" == --* ]]; do
    case "$1" in
      --allow) allow="$2" ;;
      --allow-bound) bound="$2" ;;
      *) echo "bench/rt-gate.sh: 知らない引数 $1" >&2; exit 2 ;;
    esac
    shift 2
  done
  local root base pairs new back out i
  root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
  base="$(git -C "$root" rev-parse --verify "$1^{commit}")"
  pairs="${2:-5}"
  if [[ -n "$(git -C "$root" status --porcelain)" ]]; then
    echo "bench/rt-gate.sh: 作業ツリーに変更がある" >&2
    exit 1
  fi
  new="$(git -C "$root" rev-parse HEAD)"
  back="$(git -C "$root" symbolic-ref -q --short HEAD || echo "$new")"
  out="$(mktemp -d)"
  # 終わりには main の局所変数がもうないので、ここで値を埋めておく
  # shellcheck disable=SC2064
  trap "git -C '$root' checkout -q '$back'" EXIT
  for i in $(seq "$pairs"); do
    git -C "$root" checkout -q "$base"
    "$root/bench/rt.sh" >"$out/base.$i"
    git -C "$root" checkout -q "$new"
    "$root/bench/rt.sh" >"$out/new.$i"
  done
  compare "$out" "$pairs" "$allow" "$bound"
}

# compare <表を置いたディレクトリ> <組の数> <許す行> <許す行の上限>
compare() {
  awk -v pairs="$2" -v allow="$3" -v bound="$4" '
    function row(line,    f) {
      split(line, f, "|")
      gsub(/[` ]/, "", f[2]); gsub(/ /, "", f[3])
      name = f[2]; count = f[3]
      return name != "" && count ~ /^[0-9]+$/
    }
    function median(a, n,    i, j, t) {
      for (i = 1; i <= n; i++) for (j = i + 1; j <= n; j++) if (a[j] < a[i]) { t = a[i]; a[i] = a[j]; a[j] = t }
      return a[int((n + 1) / 2)]
    }
    BEGIN { split(allow, names, " "); for (k in names) allowed[names[k]] = 1 }
    FNR == 1 { split(FILENAME, parts, "/"); split(parts[length(parts)], tag, "."); side = tag[1]; pair = tag[2] }
    row($0) { if (!((side, pair, name) in seen)) { seen[side, pair, name] = 1; v[side, pair, name] = count; if (side == "base" && pair == 1) order[++rows] = name } }
    END {
      bad = 0; sum = 0
      print "| 行 | 差の中央値 (%) | 差の中央値 (命令) | 門 |"
      print "|---|---:|---:|---|"
      for (r = 1; r <= rows; r++) {
        name = order[r]; n = 0
        for (p = 1; p <= pairs; p++)
          if ((("new", p, name) in v)) {
            n++
            abs[n] = v["new", p, name] - v["base", p, name]
            pct[n] = 100 * abs[n] / v["base", p, name]
          }
        if (n == 0) continue
        m = median(pct, n); ma = median(abs, n)
        gated = !(name == "empty" || name ~ /^println-/ || name ~ /^read-/ || name ~ /^stdout-/)
        limit = (name in allowed) ? bound : 0.1
        verdict = gated ? (m <= limit ? ((name in allowed) && m > 0.1 ? "許した" : "ok") : "超えた") : "見ない"
        if (gated) { sum += ma; if (m > limit) bad = 1 }
        printf "| `%s` | %+.3f | %+d | %s |\n", name, m, ma, verdict
      }
      printf "\n門の行の差の和: %+d 命令\n", sum
      if (allow != "" && sum >= 0) { print "和が負でない"; bad = 1 }
      exit bad
    }' "$1"/base.* "$1"/new.*
}

main "$@"
exit
