#!/usr/bin/env bash
# Nyx Capsule : script exécuté DANS la distribution WSL dédiée « NyxCapsule ».
# Appelé par windows/nyx.ps1 — ne pas lancer à la main depuis Windows.
#
#   provision <dossier Téléchargements Windows>   (root) paquets, utilisateur, Rust
#   build                                          (nyx)  compile Nyx en release
#   run                                            (nyx)  lance Nyx
set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
NYX_USER=nyx
TARGET="/home/$NYX_USER/.nyx/target"
BIN="$TARGET/release/nyx"

provision() {
    local win_downloads="$1"
    export DEBIAN_FRONTEND=noninteractive
    apt-get update -q
    apt-get install -yq --no-install-recommends \
        build-essential pkg-config curl ca-certificates git \
        libwebkit2gtk-4.1-dev libgtk-3-dev libsqlcipher-dev \
        fonts-noto-core fonts-noto-color-emoji

    id "$NYX_USER" &>/dev/null || useradd -m -s /bin/bash "$NYX_USER"

    # Utilisateur par défaut + PATH Windows coupé (évite le cargo Windows
    # et accélère chaque lancement).
    printf '[user]\ndefault=%s\n\n[interop]\nappendWindowsPath=false\n' "$NYX_USER" > /etc/wsl.conf

    # Pont Téléchargements : ~/Downloads de la capsule = dossier Windows.
    local link="/home/$NYX_USER/Downloads"
    local dest
    dest="$(wslpath -u "$win_downloads")"
    if [ -L "$link" ] || [ ! -e "$link" ]; then
        ln -sfn "$dest" "$link"
        chown -h "$NYX_USER:$NYX_USER" "$link"
    fi

    install_desktop_entry

    su - "$NYX_USER" -c 'command -v cargo >/dev/null || [ -x ~/.cargo/bin/cargo ] ||
        curl --proto =https --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal'
}

# Lanceur + icône : WSLg les lit pour afficher le logo Nyx dans la barre des
# tâches (au lieu du pingouin) et ajoute « Nyx » au menu Démarrer de Windows.
install_desktop_entry() {
    local id=io.nyx.browser
    install -Dm644 "$REPO/assets/icons/nyx.png" "/usr/share/icons/hicolor/512x512/apps/$id.png"
    install -Dm644 "$REPO/assets/icons/nyx.png" "/usr/share/pixmaps/$id.png"
    cat > "/usr/share/applications/$id.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Nyx
Comment=Navigateur Nyx
Exec=$BIN %U
Icon=$id
StartupWMClass=$id
Categories=Network;WebBrowser;
Terminal=false
EOF
    command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -qf /usr/share/icons/hicolor || true
}

build() {
    # shellcheck source=/dev/null
    source "$HOME/.cargo/env"
    cd "$REPO"
    # target/ dans le disque Linux de la capsule : 5 à 10× plus rapide que /mnt/c.
    CARGO_TARGET_DIR="$TARGET" cargo build --release --bin nyx
}

run() {
    [ -x "$BIN" ] || { echo "Nyx n'est pas compilé : lance 'nyx.ps1 build'." >&2; exit 1; }
    cd "$HOME"
    exec "$BIN"
}

case "${1:-}" in
    provision) provision "${2:?dossier Téléchargements manquant}" ;;
    build)     build ;;
    run)       run ;;
    *) echo "usage : capsule.sh provision <dir> | build | run" >&2; exit 2 ;;
esac
