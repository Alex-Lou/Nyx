#!/usr/bin/env bash
# bootstrap.sh — clone le projet Nyx dans le dossier courant
# Lancer depuis : /mnt/c/Users/34643/Desktop/Brol/Nyx
# Usage : bash bootstrap.sh
set -e

echo "═══════════════════════════════════════"
echo "  Nyx Browser — Bootstrap"
echo "═══════════════════════════════════════"

# ── Vérifications ────────────────────────────────────────────────────────────

check() {
    if ! command -v "$1" &>/dev/null; then
        echo "✗ $1 non trouvé — installe-le d'abord"
        exit 1
    fi
    echo "✓ $1"
}

echo ""
echo "→ Vérification des dépendances..."
check cargo
check git
check pkg-config

# Vérifie les libs système
for lib in webkit2gtk-4.1 gtk+-3.0 sqlcipher; do
    if pkg-config --exists "$lib" 2>/dev/null; then
        echo "✓ lib $lib"
    else
        echo "✗ lib $lib manquante"
        echo "  → sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libsqlcipher-dev"
        exit 1
    fi
done

echo ""
echo "→ Toutes les dépendances OK."
echo ""

# ── Fonts (recommandé, non bloquant) ─────────────────────────────────────────

if fc-list | grep -qi "JetBrains Mono"; then
    echo "✓ JetBrains Mono installée"
else
    echo "⚠  JetBrains Mono non trouvée (l'URL bar sera moins belle)"
    echo "   Pour l'installer : https://www.jetbrains.com/lp/mono/"
fi

# ── Build ─────────────────────────────────────────────────────────────────────

echo ""
echo "→ Compilation (première fois = longue, patience…)"
cargo build 2>&1

echo ""
echo "═══════════════════════════════════════"
echo "  ✓ Build OK"
echo ""
echo "  Lancer Nyx :"
echo "  cargo run -p nyx"
echo "═══════════════════════════════════════"
