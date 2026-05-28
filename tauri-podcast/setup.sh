#!/bin/bash
# ─────────────────────────────────────────────────────────────────────────────
# Transcriber-kun — macOS Setup Helper
# Installs system dependencies and Python packages.
# ─────────────────────────────────────────────────────────────────────────────

set -e
CYAN='\033[0;36m'; GREEN='\033[0;32m'; RED='\033[0;31m'; YELLOW='\033[1;33m'; NC='\033[0m'
export PATH="/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/bin:/usr/local/sbin:/opt/local/bin:/opt/local/sbin:$PATH"

echo -e "${CYAN}🎧 Transcriber-kun — Setup${NC}"
echo "──────────────────────────────────"

# ── 1. Check Python 3.10+ ──────────────────────────────────────────────────
echo -e "\n${CYAN}1. Checking Python...${NC}"
PYTHON=""

python_supported() {
  "$1" -c 'import sys; raise SystemExit(0 if sys.version_info >= (3, 10) else 1)' 2>/dev/null
}

python_version() {
  "$1" -c 'import sys; print(f"{sys.version_info.major}.{sys.version_info.minor}.{sys.version_info.micro}")' 2>/dev/null
}

PYTHON_CANDIDATES=()
for ver in 3.14 3.13 3.12 3.11 3.10; do
  PYTHON_CANDIDATES+=(
    "/opt/homebrew/bin/python${ver}"
    "/usr/local/bin/python${ver}"
    "/opt/homebrew/opt/python@${ver}/bin/python${ver}"
    "/opt/homebrew/opt/python@${ver}/bin/python3"
    "/usr/local/opt/python@${ver}/bin/python${ver}"
    "/usr/local/opt/python@${ver}/bin/python3"
    "/opt/local/bin/python${ver}"
    "/Library/Frameworks/Python.framework/Versions/${ver}/bin/python3"
    "python${ver}"
  )
done
PYTHON_CANDIDATES+=("python3" "python")

for cmd in "${PYTHON_CANDIDATES[@]}"; do
  if [[ "$cmd" = /* ]]; then
    [ -x "$cmd" ] || continue
    CANDIDATE="$cmd"
  else
    CANDIDATE="$(command -v "$cmd" 2>/dev/null || true)"
    [ -n "$CANDIDATE" ] || continue
  fi

  if python_supported "$CANDIDATE"; then
    PYTHON="$CANDIDATE"
    VER=$(python_version "$CANDIDATE")
    echo -e "${GREEN}✅ Found Python $VER at $PYTHON${NC}"
    break
  fi
done

if [ -z "$PYTHON" ]; then
  echo -e "${RED}❌ Python 3.10+ not found.${NC}"
  echo "   Please install from: https://www.python.org/downloads/"
  echo "   Or via Homebrew: brew install python@3.11"
  exit 1
fi

# ── 2. Check/install ffmpeg ───────────────────────────────────────────────
echo -e "\n${CYAN}2. Checking ffmpeg...${NC}"
if command -v ffmpeg &>/dev/null && command -v ffprobe &>/dev/null; then
  echo -e "${GREEN}✅ ffmpeg/ffprobe available${NC}"
else
  echo -e "${YELLOW}⚠️  ffmpeg/ffprobe not found.${NC}"
  if command -v brew &>/dev/null; then
    echo "   Installing via Homebrew..."
    brew install ffmpeg
    echo -e "${GREEN}✅ ffmpeg installed${NC}"
  else
    echo -e "${YELLOW}   Homebrew not found. Install ffmpeg manually:${NC}"
    echo "   /bin/bash -c \"\$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)\""
    echo "   brew install ffmpeg"
  fi
fi

# ── 3. Create venv and install packages ───────────────────────────────────
APP_DATA_DIR="$HOME/Library/Application Support/com.transcriberkun.app"
VENV_DIR="$APP_DATA_DIR/venv"

echo -e "\n${CYAN}3. Setting up Python environment...${NC}"
if [ -d "$VENV_DIR" ] && { [ ! -x "$VENV_DIR/bin/python3" ] || ! python_supported "$VENV_DIR/bin/python3"; }; then
  echo "   Removing outdated venv at: $VENV_DIR"
  rm -rf "$VENV_DIR"
fi

if [ ! -d "$VENV_DIR" ]; then
  echo "   Creating venv at: $VENV_DIR"
  mkdir -p "$APP_DATA_DIR"
  "$PYTHON" -m venv "$VENV_DIR"
fi

echo "   Installing Python packages..."
"$VENV_DIR/bin/python3" -m pip install --upgrade pip -q
"$VENV_DIR/bin/python3" -m pip install -r requirements.txt

echo -e "${GREEN}✅ Python packages installed${NC}"

# ── 4. Check Rust + Tauri ────────────────────────────────────────────────
echo -e "\n${CYAN}4. Checking build tools...${NC}"
if ! command -v cargo &>/dev/null; then
  echo -e "${YELLOW}⚠️  Rust/Cargo not found. Install from: https://rustup.rs${NC}"
  echo "   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
else
  echo -e "${GREEN}✅ Rust $(rustc --version | cut -d' ' -f2)${NC}"
fi

if ! cargo tauri --version &>/dev/null; then
  echo "   Installing tauri-cli..."
  cargo install tauri-cli --version "^2" --locked
fi
echo -e "${GREEN}✅ Tauri CLI ready${NC}"

# ── Done ──────────────────────────────────────────────────────────────────
echo -e "\n${GREEN}✅ Setup complete!${NC}"
echo ""
echo "To run in development mode:"
echo "  cd tauri-podcast && cargo tauri dev"
echo ""
echo "To build the .app:"
echo "  cd tauri-podcast && cargo tauri build"
echo ""
