#!/bin/bash
# クラウドのセッションで `nix develop` を使えるようにする SessionStart フック。
# クラウドの proxy は GitHub の archive の取得を断るが、git の clone は通す。flake.lock の `github:` の input は
# archive で取るので、同じ rev を git で取って store に置く。input の store の道は narHash で決まるので、
# 置いた後の `nix develop` は archive を取りに行かない (docs/implementation/testing.md の「クラウドのセッション」)。
set -euo pipefail

if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

cd "$CLAUDE_PROJECT_DIR"

# 各行は owner、repo、rev、narHash。同じ input を複数の node が指すことがあるので重複を除く
python3 -I -c '
import json, sys
nodes = json.load(open(sys.argv[1]))["nodes"]
seen = set()
for node in nodes.values():
    locked = node.get("locked")
    if not locked or locked.get("type") != "github":
        continue
    row = (locked["owner"], locked["repo"], locked["rev"], locked["narHash"])
    if row not in seen:
        seen.add(row)
        print(*row)
' flake.lock | while read -r owner repo rev nar_hash; do
  got=$(nix flake prefetch --json "git+https://github.com/$owner/$repo?rev=$rev&shallow=1" |
    python3 -I -c 'import json, sys; print(json.load(sys.stdin)["hash"])')
  # git で取った中身が lock の中身と違えば、store の道も違い、`nix develop` は archive を取りに行って失敗する
  if [ "$got" != "$nar_hash" ]; then
    echo "session-start: $owner/$repo@$rev の narHash が flake.lock と違う ($got != $nar_hash)" >&2
    exit 1
  fi
done

# devShell を一度組んでおく。環境の setup script からこのフックを呼ぶと、組んだものが環境のキャッシュに残る
nix develop -c true
