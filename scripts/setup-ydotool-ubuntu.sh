#!/usr/bin/env bash
# Installs ydotool and runs ydotoold as a system service whose socket is
# owned by the invoking user, so voice_input can press Ctrl+V on Wayland
# (GNOME). No re-login needed.
#
#   sudo ./scripts/setup-ydotool-ubuntu.sh
set -euo pipefail

if [[ $EUID -ne 0 || -z "${SUDO_UID:-}" ]]; then
    echo "run with sudo from your normal user: sudo $0" >&2
    exit 1
fi

SOCKET=/tmp/.ydotool_socket

apt-get install -y ydotool

cat > /etc/systemd/system/ydotoold.service <<UNIT
[Unit]
Description=ydotool daemon (uinput) for voice_input
After=systemd-udevd.service

[Service]
ExecStartPre=/sbin/modprobe uinput
ExecStart=/usr/bin/ydotoold --socket-path=${SOCKET} --socket-own=${SUDO_UID}:${SUDO_GID}
Restart=on-failure

[Install]
WantedBy=multi-user.target
UNIT

systemctl daemon-reload
systemctl enable --now ydotoold.service
sleep 1
systemctl --no-pager --lines=5 status ydotoold.service || true

# Smoke test as the user: tap Shift (harmless).
if sudo -u "#${SUDO_UID}" env YDOTOOL_SOCKET="${SOCKET}" ydotool key 42:1 42:0; then
    echo "OK: ydotool works for $(id -nu "$SUDO_UID")"
else
    echo "NG: ydotool could not reach ydotoold (see status above)" >&2
    exit 1
fi
