#!/usr/bin/env bash
# Instala el túnel SSH forward (raspihome → wallabot) como user systemd unit.
#
# Diseñado para correr EN raspihome. Mantiene `ssh -L 9123:127.0.0.1:9123 wallabot`
# vivo con auto-reconnect. Como HA corre con --network host, leer 127.0.0.1:9123
# desde el contenedor llega al gpu-monitord remoto.
#
# Requisitos previos:
#   1. SSH a wallabot funciona desde raspihome con clave dedicada (este script
#      la genera si no existe; te imprime la línea para añadir a authorized_keys
#      en wallabot).
#   2. Linger habilitado para que el unit sobreviva sin sesión activa:
#         sudo loginctl enable-linger "$USER"
#
# Uso:
#   ./install.sh           # instala y arranca
#   ./install.sh uninstall # para y desinstala

set -euo pipefail

UNIT_NAME="gpu-monitor-ha-tunnel.service"
UNIT_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
SRC_UNIT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$UNIT_NAME"
TUNNEL_KEY="$HOME/.ssh/id_ed25519_gpu_tunnel"
REMOTE_HOST="wallabot@wallabot"

case "${1:-install}" in
    install)
        if ! command -v ssh >/dev/null; then
            echo "ssh no encontrado en PATH" >&2
            exit 1
        fi
        if [ ! -f "$TUNNEL_KEY" ]; then
            echo "Generando clave dedicada para el túnel: $TUNNEL_KEY"
            ssh-keygen -t ed25519 -N "" -f "$TUNNEL_KEY" -C "raspihome-gpu-tunnel" >/dev/null
            echo
            echo "Añade esta línea a ~/.ssh/authorized_keys en $REMOTE_HOST:"
            echo
            echo "  restrict,permitopen=\"127.0.0.1:9123\" $(cat "$TUNNEL_KEY.pub")"
            echo
            echo "Comando rápido (si tienes SSH directo desde aquí a $REMOTE_HOST):"
            echo "  cat $TUNNEL_KEY.pub | ssh $REMOTE_HOST \\"
            echo "    'sed \"s|^|restrict,permitopen=\\\"127.0.0.1:9123\\\" |\" >> ~/.ssh/authorized_keys'"
            echo
            read -p "Pulsa enter cuando lo hayas hecho... "
        fi
        if ! ssh -i "$TUNNEL_KEY" -o IdentitiesOnly=yes -o BatchMode=yes \
                -o ConnectTimeout=5 -o StrictHostKeyChecking=accept-new \
                "$REMOTE_HOST" true 2>/dev/null; then
            echo "Error: la clave dedicada no autentica contra $REMOTE_HOST." >&2
            echo "       Verifica que la añadiste a ~/.ssh/authorized_keys." >&2
            exit 1
        fi
        if ! loginctl show-user "$USER" --property=Linger 2>/dev/null | grep -q "Linger=yes"; then
            echo "Aviso: linger no está habilitado para $USER." >&2
            echo "       Sin linger, el túnel se cae al cerrar la sesión." >&2
            echo "       Habilítalo con:  sudo loginctl enable-linger $USER" >&2
        fi
        mkdir -p "$UNIT_DIR"
        cp "$SRC_UNIT" "$UNIT_DIR/$UNIT_NAME"
        systemctl --user daemon-reload
        systemctl --user enable --now "$UNIT_NAME"
        echo "Instalado. Estado:"
        systemctl --user --no-pager status "$UNIT_NAME" | head -n 12 || true
        echo
        echo "Logs:  journalctl --user -u $UNIT_NAME -f"
        ;;
    uninstall)
        systemctl --user disable --now "$UNIT_NAME" 2>/dev/null || true
        rm -f "$UNIT_DIR/$UNIT_NAME"
        systemctl --user daemon-reload
        echo "Desinstalado."
        ;;
    *)
        echo "Uso: $0 [install|uninstall]" >&2
        exit 2
        ;;
esac
