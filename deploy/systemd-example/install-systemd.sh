#!/usr/bin/env bash
# EXAMPLE / TESTING ONLY — see README.md in this directory.
#
# Renders torpc-tor.service.template and torpc-daemon.service.template by
# substituting ${USER} and ${TORPC_HOME}, then either prints the result to
# stdout or installs into /etc/systemd/system/ (with sudo).
#
# This is a minimal scaffold for testing systemd integration on a dev box
# or staging host. It is NOT a production-blessed deployment path. The
# templates carry a reasonable hardening baseline but no high-availability,
# secret management, or upgrade story.
#
# Usage:
#   deploy/systemd-example/install-systemd.sh                 # print rendered units
#   deploy/systemd-example/install-systemd.sh --install       # render + sudo install + daemon-reload
#   deploy/systemd-example/install-systemd.sh --user alice --home /srv/torpc --install

set -euo pipefail

USER_NAME="${USER:-}"
# Default to the repo root (two levels up from this script).
TORPC_HOME="$(cd "$(dirname "$0")/../.." && pwd)"
DO_INSTALL=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --user)    USER_NAME="$2"; shift 2 ;;
        --home)    TORPC_HOME="$2"; shift 2 ;;
        --install) DO_INSTALL=true; shift ;;
        -h|--help)
            sed -n '2,12p' "$0"
            exit 0
            ;;
        *) echo "unknown arg: $1" >&2; exit 1 ;;
    esac
done

if [[ -z "$USER_NAME" ]]; then
    echo "USER not set in environment; pass --user <name>" >&2
    exit 1
fi

render() {
    local template="$1"
    sed -e "s|\${USER}|${USER_NAME}|g" \
        -e "s|\${TORPC_HOME}|${TORPC_HOME}|g" \
        "$template"
}

# Templates live next to this script.
cd "$(dirname "$0")"

for template in torpc-tor.service.template torpc-daemon.service.template; do
    if [[ ! -f "$template" ]]; then
        echo "missing $template" >&2
        exit 1
    fi
    target="$(basename "$template" .template)"
    rendered="$(render "$template")"

    if $DO_INSTALL; then
        printf '%s\n' "$rendered" | sudo tee "/etc/systemd/system/$target" >/dev/null
        echo "installed /etc/systemd/system/$target"
    else
        echo "# ----- $target -----"
        printf '%s\n\n' "$rendered"
    fi
done

if $DO_INSTALL; then
    sudo systemctl daemon-reload
    echo "systemctl daemon-reload done. Enable with:"
    echo "  sudo systemctl enable --now torpc-tor torpc-daemon"
fi
