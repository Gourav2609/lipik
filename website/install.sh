#!/bin/sh
# Installs the latest Lipik release into /Applications:
#   curl -fsSL https://lipik.pages.dev/install.sh | sh
# Settings, history and meetings live in ~/Library and are kept.
# LIPIK_DEMO=1 plays the whole thing without downloading or installing.
set -eu

REPO="Gourav2609/lipik"
URL="${LIPIK_DMG_URL:-https://github.com/$REPO/releases/latest/download/Lipik.dmg}"
DEMO="${LIPIK_DEMO:-}"
LEAF='H"f1a4bea1f48a60a814691e428ac100722d10a895"'
APP_BIN='Lipik.app/Contents/MacOS/lipik$'
# LIPIK_INSTALL_DIR is for testing: it also limits the running-app check to
# that folder so the real Lipik is left alone.
[ -n "${LIPIK_INSTALL_DIR:-}" ] && APP_BIN="$LIPIK_INSTALL_DIR/$APP_BIN"
CURL_OPTS="--retry 3 --connect-timeout 15 --speed-time 30 --speed-limit 20000"

# ---------- look ----------

FANCY=""
if [ -t 1 ] && [ "${TERM:-dumb}" != dumb ] && [ -z "${NO_COLOR:-}" ]; then
  FANCY=1
fi

if [ -n "$FANCY" ]; then
  case "${COLORTERM:-}" in
    truecolor | 24bit)
      G1='\033[38;2;178;228;198m' G2='\033[38;2;150;214;178m' G3='\033[38;2;122;198;156m'
      G4='\033[38;2;96;180;136m' G5='\033[38;2;74;160;118m' G6='\033[38;2;58;138;102m'
      ;;
    *)
      G1='\033[38;5;157m' G2='\033[38;5;121m' G3='\033[38;5;115m'
      G4='\033[38;5;79m' G5='\033[38;5;72m' G6='\033[38;5;29m'
      ;;
  esac
  B='\033[1m' D='\033[2m' R='\033[0m' RED='\033[31m'
else
  G1='' G2='' G3='' G4='' G5='' G6='' B='' D='' R='' RED=''
fi

pause() { [ -n "$FANCY" ] && sleep "$1" || true; }

say() { printf "%b\n" "$1"; }

type_out() {
  if [ -z "$FANCY" ]; then
    printf "%b%s%b\n" "$2" "$1" "$R"
    return
  fi
  printf "%b" "$2"
  printf '%s\n' "$1" | fold -w1 | while IFS= read -r c; do
    printf '%s' "${c:- }"
    sleep 0.012
  done
  printf "%b\n" "$R"
}

logo() {
  say ""
  for line in \
    "$G1  ██╗     ██╗██████╗ ██╗██╗  ██╗" \
    "$G2  ██║     ██║██╔══██╗██║██║ ██╔╝" \
    "$G3  ██║     ██║██████╔╝██║█████╔╝ " \
    "$G4  ██║     ██║██╔═══╝ ██║██╔═██╗ " \
    "$G5  ███████╗██║██║     ██║██║  ██╗" \
    "$G6  ╚══════╝╚═╝╚═╝     ╚═╝╚═╝  ╚═╝"; do
    say "$line$R"
    pause 0.06
  done
  say "  ${D}लिपिक, Hindi for scribe$R"
  say ""
  type_out "  Free, open-source dictation for your Mac." "$B"
  type_out "  Speech to text on your Mac. Your voice never leaves it." "$D"
  say ""
}

# ---------- steps ----------

SPIN="⠋ ⠙ ⠹ ⠸ ⠼ ⠴ ⠦ ⠧ ⠇ ⠏"

# step "label" command...: spinner while the command runs, then a tick.
step() {
  label="$1"
  shift
  if [ -z "$FANCY" ]; then
    printf '  %s... ' "$label"
    "$@" || { say "failed"; return 1; }
    say "done"
    return
  fi
  "$@" &
  pid=$!
  while kill -0 "$pid" 2>/dev/null; do
    for f in $SPIN; do
      printf "\r  %b%s%b %s" "$G3" "$f" "$R" "$label"
      sleep 0.07
      kill -0 "$pid" 2>/dev/null || break
    done
  done
  if wait "$pid"; then
    printf "\r  %b✓%b %s\033[K\n" "$G3" "$R" "$label"
  else
    printf "\r  %b✗%b %s\033[K\n" "$RED" "$R" "$label"
    return 1
  fi
}

fail() {
  say ""
  say "  $RED$1$R"
  say ""
  exit 1
}

mb() { awk -v b="$1" 'BEGIN { printf "%.1f", b / 1048576 }'; }

bar() {
  done_=$1 total=$2 width=28
  fill=$((done_ * width / total))
  [ "$fill" -gt "$width" ] && fill=$width
  i=0 s=""
  while [ $i -lt $fill ]; do s="${s}━"; i=$((i + 1)); done
  t=""
  while [ $i -lt $width ]; do t="${t}━"; i=$((i + 1)); done
  printf "\r  %b↓%b Downloading Lipik%s  %b%s%b%s%b  %s / %s MB\033[K" \
    "$G3" "$R" "$VERSION_LABEL" "$G3" "$s" "$D" "$t" "$R" "$(mb "$done_")" "$(mb "$total")"
}

download() {
  if [ -n "$DEMO" ]; then
    total=$(curl -fsIL $CURL_OPTS "$URL" 2>/dev/null | awk 'tolower($1) == "content-length:" { v = $2 } END { gsub("\r", "", v); print v + 0 }') || total=0
    [ "${total:-0}" -gt 0 ] || total=23068672
    got=0
    while [ -n "$FANCY" ] && [ $got -lt $total ]; do
      bar "$got" "$total"
      got=$((got + 1048576))
      sleep 0.05
    done
    [ -n "$FANCY" ] && bar "$total" "$total"
  else
    total=$(curl -fsIL $CURL_OPTS "$URL" | awk 'tolower($1) == "content-length:" { v = $2 } END { gsub("\r", "", v); print v + 0 }')
    curl -fsSL $CURL_OPTS -o "$TMP/Lipik.dmg" "$URL" &
    pid=$!
    if [ -n "$FANCY" ] && [ "${total:-0}" -gt 0 ]; then
      while kill -0 "$pid" 2>/dev/null; do
        bar "$(stat -f%z "$TMP/Lipik.dmg" 2>/dev/null || echo 0)" "$total"
        sleep 0.08
      done
    fi
    wait "$pid" || fail "The download failed. Check your connection and try again."
    total=$(stat -f%z "$TMP/Lipik.dmg")
    [ -n "$FANCY" ] && bar "$total" "$total"
  fi
  if [ -n "$FANCY" ]; then
    printf "\r  %b✓%b Downloaded Lipik%s, %s MB\033[K\n" "$G3" "$R" "$VERSION_LABEL" "$(mb "$total")"
  else
    say "  Downloaded Lipik$VERSION_LABEL."
  fi
}

# Only the main app counts: the `lipik --mcp` helpers that AI apps start keep
# running, and replacing the app under them is fine.
lipik_open() { pgrep -qf "$APP_BIN"; }

close_lipik() {
  if [ -n "${LIPIK_INSTALL_DIR:-}" ]; then
    pkill -f "$APP_BIN" || true
  else
    osascript -e 'tell application id "com.gourav.lipik" to quit' >/dev/null 2>&1 || true
  fi
  i=0
  while lipik_open; do
    i=$((i + 1))
    [ $i -gt 40 ] && return 1
    sleep 0.25
  done
}

check_mac() {
  [ -n "$DEMO" ] && { sleep 0.6; return 0; }
  [ "$(uname -s)" = Darwin ] && [ "$(sysctl -n hw.optional.arm64 2>/dev/null)" = 1 ]
}

latest_version() {
  curl -fsS --connect-timeout 15 --max-time 30 "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null |
    sed -n 's/.*"tag_name": *"v\{0,1\}\([^"]*\)".*/\1/p' | head -n 1 >"$TMP/version" || true
}

check_app() {
  [ -n "$DEMO" ] && { sleep 0.4; return 0; }
  mkdir "$MNT"
  hdiutil attach -quiet -nobrowse -readonly -mountpoint "$MNT" "$TMP/Lipik.dmg" &&
    codesign --verify --deep --strict "$MNT/Lipik.app" 2>/dev/null &&
    codesign --verify -R="identifier \"com.gourav.lipik\" and certificate leaf = $LEAF" "$MNT/Lipik.app" 2>/dev/null
}

# Copy next to the old app first, then swap, so a failure never leaves a
# half-deleted Lipik behind.
install_app() {
  [ -n "$DEMO" ] && { sleep 1.2; return 0; }
  new="$DEST/.Lipik.app.new.$$" old="$DEST/.Lipik.app.old.$$"
  ditto "$MNT/Lipik.app" "$new" 2>/dev/null || { rm -rf "$new" 2>/dev/null; return 1; }
  if [ -e "$DEST/Lipik.app" ]; then
    mv "$DEST/Lipik.app" "$old" 2>/dev/null || { rm -rf "$new" 2>/dev/null; return 1; }
  fi
  if ! mv "$new" "$DEST/Lipik.app" 2>/dev/null; then
    [ -e "$old" ] && mv "$old" "$DEST/Lipik.app" 2>/dev/null
    rm -rf "$new" 2>/dev/null
    return 1
  fi
  if [ -e "$old" ]; then
    chmod -R u+w "$old" 2>/dev/null || true
    rm -rf "$old" 2>/dev/null || true
  fi
}

# A few seconds of the pill's sound bars, then the first thing to try.
listening() {
  [ -z "$FANCY" ] && return 0
  frames=0
  while [ $frames -lt 36 ]; do
    line="" n=0
    for h in 1 2 4 6 7 5 3 2 4 6 7 6 4 2 1 3 5 7 6 3; do
      k=$(((h + frames + n) % 8))
      case $k in
        0) c=▁ ;; 1) c=▂ ;; 2) c=▃ ;; 3) c=▄ ;; 4) c=▅ ;; 5) c=▆ ;; 6) c=▇ ;; *) c=█ ;;
      esac
      line="$line$c"
      n=$((n + 1))
    done
    printf "\r  %b%s%b  %blistening%b\033[K" "$G3" "$line" "$R" "$D" "$R"
    sleep 0.06
    frames=$((frames + 1))
  done
  printf "\r\033[K"
}

# ---------- run ----------

TMP=$(mktemp -d)
MNT="$TMP/mnt"
cleanup() {
  [ -n "$FANCY" ] && printf '\033[?25h'
  hdiutil detach -quiet "$MNT" 2>/dev/null || true
  rm -rf "$TMP"
}
trap cleanup EXIT
trap 'exit 130' INT TERM
[ -n "$FANCY" ] && printf '\033[?25l'

logo

step "Checking your Mac" check_mac ||
  fail "Lipik needs a Mac with Apple silicon (M1 or later)."
if [ -z "$DEMO" ] && lipik_open; then
  step "Closing the open Lipik" close_lipik ||
    fail "Couldn't close Lipik. Quit it from its menu bar icon, then run this again."
fi

step "Finding the latest version" latest_version
VERSION=$(cat "$TMP/version" 2>/dev/null || true)
VERSION_LABEL=""
[ -n "$VERSION" ] && VERSION_LABEL=" $VERSION"

download

step "Checking it's the real Lipik" check_app ||
  fail "This download isn't signed by Lipik. Nothing was installed. Try again later."

DEST="${LIPIK_INSTALL_DIR:-/Applications}"
[ -w "$DEST" ] || { DEST="$HOME/Applications"; mkdir -p "$DEST"; }
step "Moving it into $DEST" install_app ||
  fail "Couldn't replace Lipik in $DEST. Quit Lipik and run this again, or drag Lipik.dmg into Applications."

say ""
listening
say "  $B${G2}Lipik$VERSION_LABEL is ready.$R"
say ""
say "  Hold ${B}fn$R and start talking. Let go, and your words appear."
say "  ${D}lipik.pages.dev$R"
say ""

[ -z "$DEMO" ] && [ -z "${LIPIK_INSTALL_DIR:-}" ] && open "$DEST/Lipik.app"
exit 0
