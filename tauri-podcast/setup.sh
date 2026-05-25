#!/bin/bash
# ─────────────────────────────────────────────────────────────────────────────
# Transcriber-kun — macOS Setup Helper
# Installs system dependencies and Python packages.
# ─────────────────────────────────────────────────────────────────────────────

set -e
CYAN='\033[0;36m'; GREEN='\033[0;32m'; RED='\033[0;31m'; YELLOW='\033[1;33m'; NC='\033[0m'

echo -e "${CYAN}🎧 Transcriber-kun — Setup${NC}"
echo "──────────────────────────────────"

# ── 1. Check Python 3.10+ ──────────────────────────────────────────────────
echo -e "\n${CYAN}1. Checking Python...${NC}"
PYTHON=""
for cmd in python3 python; do
  if command -v "$cmd" &>/dev/null; then
    VER=$($cmd -c "import sys; print(f'{sys.version_info.major}.{sys.version_info.minor}')" 2>/dev/null)
    MAJOR=$(echo $VER | cut -d. -f1)
    MINOR=$(echo $VER | cut -d. -f2)
    if [ "$MAJOR" -ge 3 ] && [ "$MINOR" -ge 10 ]; then
      PYTHON="$cmd"
      echo -e "${GREEN}✅ Found Python $VER${NC}"
      break
    fi
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
if command -v ffmpeg &>/dev/null; then
  echo -e "${GREEN}✅ ffmpeg available${NC}"
else
  echo -e "${YELLOW}⚠️  ffmpeg not found.${NC}"
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
if [ ! -d "$VENV_DIR" ]; then
  echo "   Creating venv at: $VENV_DIR"
  mkdir -p "$APP_DATA_DIR"
  $PYTHON -m venv "$VENV_DIR"
fi

echo "   Installing Python packages..."
"$VENV_DIR/bin/pip" install --upgrade pip -q
"$VENV_DIR/bin/pip" install faster-whisper google-generativeai python-dotenv

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
