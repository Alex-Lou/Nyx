#!/usr/bin/env bash
# Met à jour EasyList + EasyPrivacy bundlées (miroir GitHub uBlock).
# Usage : bash scripts/update-filterlists.sh && cargo build
set -euo pipefail

cd "$(dirname "$0")/.."
MIRROR="https://raw.githubusercontent.com/uBlockOrigin/uAssetsCDN/main/thirdparties"

for list in easylist easyprivacy; do
    echo "→ $list"
    curl -fsSL --max-time 60 "$MIRROR/$list.txt" | gzip -9 > "assets/filterlists/$list.txt.gz"
done

echo "✓ Listes mises à jour — recompile, puis supprime le cache du filtre :"
echo "  rm -rf ~/.cache/nyx/filters"
