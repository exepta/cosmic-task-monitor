#!/usr/bin/env bash
set -euo pipefail

APP_ID="com.github.exepta.cosmic-task-monitor"
APP_NAME="cosmic-task-monitor"
ROOT="${HOME}/.local"
BIN_DIR="${ROOT}/bin"
APPS_DIR="${ROOT}/share/applications"
APPDATA_DIR="${ROOT}/share/appdata"
ICON_DIR="${ROOT}/share/icons/hicolor/scalable/apps"

cargo build --release

install -Dm0755 "target/release/${APP_NAME}" "${BIN_DIR}/${APP_NAME}"
install -Dm0644 "resources/app.metainfo.xml" "${APPDATA_DIR}/${APP_ID}.metainfo.xml"
install -Dm0644 \
  "resources/icons/hicolor/scalable/apps/${APP_ID}.svg" \
  "${ICON_DIR}/${APP_ID}.svg"

mkdir -p "${APPS_DIR}"
sed "s|^Exec=.*|Exec=${BIN_DIR}/${APP_NAME} %F|" "resources/app.desktop" \
  > "${APPS_DIR}/${APP_ID}.desktop"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "${APPS_DIR}" >/dev/null 2>&1 || true
fi

if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q "${ROOT}/share/icons/hicolor" >/dev/null 2>&1 || true
fi

echo "Installed ${APP_NAME} for the current user."
echo "Launcher: ${APPS_DIR}/${APP_ID}.desktop"
echo "Binary: ${BIN_DIR}/${APP_NAME}"
