#!/usr/bin/env bash
set -euo pipefail

ROOTDIR="${1:-}"
PREFIX="${2:-/usr}"
CARGO_TARGET_DIR="${3:-target}"
APP_NAME="${4:-cosmic-task-monitor}"
APP_ID="${5:-com.github.exepta.cosmic-task-monitor}"

BASE_DIR="$(realpath -m "${ROOTDIR}${PREFIX}")"
BIN_SRC="${CARGO_TARGET_DIR}/release/${APP_NAME}"
DESKTOP_SRC="resources/app.desktop"
APPDATA_SRC="resources/app.metainfo.xml"
ICON_SRC="resources/icons/hicolor/scalable/apps/${APP_ID}.svg"

BIN_DST="${BASE_DIR}/bin/${APP_NAME}"
DESKTOP_DST="${BASE_DIR}/share/applications/${APP_ID}.desktop"
APPDATA_DST="${BASE_DIR}/share/appdata/${APP_ID}.metainfo.xml"
ICON_DST="${BASE_DIR}/share/icons/hicolor/scalable/apps/${APP_ID}.svg"

for src in "${BIN_SRC}" "${DESKTOP_SRC}" "${APPDATA_SRC}" "${ICON_SRC}"; do
  if [ ! -f "${src}" ]; then
    echo "Missing required file: ${src}" >&2
    exit 1
  fi
done

first_existing_parent() {
  local path="$1"
  local parent
  parent="$(dirname "${path}")"
  while [ ! -d "${parent}" ]; do
    parent="$(dirname "${parent}")"
  done
  printf '%s\n' "${parent}"
}

needs_privilege=0
for dst in "${BIN_DST}" "${DESKTOP_DST}" "${APPDATA_DST}" "${ICON_DST}"; do
  parent="$(first_existing_parent "${dst}")"
  if [ ! -w "${parent}" ]; then
    needs_privilege=1
    break
  fi
done

INSTALL_CMD=(install)
if [ "${EUID}" -ne 0 ] && [ "${needs_privilege}" -eq 1 ]; then
  if command -v sudo >/dev/null 2>&1; then
    INSTALL_CMD=(sudo install)
  else
    echo "Install destination requires elevated privileges, but 'sudo' is unavailable." >&2
    exit 1
  fi
fi

"${INSTALL_CMD[@]}" -Dm0755 "${BIN_SRC}" "${BIN_DST}"
"${INSTALL_CMD[@]}" -Dm0644 "${DESKTOP_SRC}" "${DESKTOP_DST}"
"${INSTALL_CMD[@]}" -Dm0644 "${APPDATA_SRC}" "${APPDATA_DST}"
"${INSTALL_CMD[@]}" -Dm0644 "${ICON_SRC}" "${ICON_DST}"

echo "Installed ${APP_NAME} into ${BASE_DIR}."
