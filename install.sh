#!/bin/bash
# ═══════════════════════════════════════════════════════════════════════════════
# ICE COLD v35 - AUTO INSTALLER FOR TERMUX
# One-command installation script
# ═══════════════════════════════════════════════════════════════════════════════

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
MAGENTA='\033[0;35m'
NC='\033[0m' # No Color

echo -e "${MAGENTA}"
echo "╔═══════════════════════════════════════════════════════════╗"
echo "║         ICE COLD v35 - RUST TURBO INSTALLER               ║"
echo "║              Optimized for Termux/Android                 ║"
echo "╚═══════════════════════════════════════════════════════════╝"
echo -e "${NC}"

# Detect environment
IS_TERMUX=false
if [ -n "$PREFIX" ] && [[ "$PREFIX" == *"com.termux"* ]]; then
    IS_TERMUX=true
    echo -e "${GREEN}[✓] Termux detected${NC}"
elif [ -d "/data/data/com.termux" ]; then
    IS_TERMUX=true
    echo -e "${GREEN}[✓] Termux detected${NC}"
else
    echo -e "${YELLOW}[*] Running on standard Linux${NC}"
fi

# Package manager
if [ "$IS_TERMUX" = true ]; then
    PKG_MANAGER="pkg"
    PKG_UPDATE="pkg update -y && pkg upgrade -y"
    INSTALL_CMD="pkg install -y"
else
    if command -v apt-get &> /dev/null; then
        PKG_MANAGER="apt-get"
        PKG_UPDATE="sudo apt-get update"
        INSTALL_CMD="sudo apt-get install -y"
    elif command -v dnf &> /dev/null; then
        PKG_MANAGER="dnf"
        PKG_UPDATE="sudo dnf check-update || true"
        INSTALL_CMD="sudo dnf install -y"
    elif command -v pacman &> /dev/null; then
        PKG_MANAGER="pacman"
        PKG_UPDATE="sudo pacman -Sy"
        INSTALL_CMD="sudo pacman -S --noconfirm"
    else
        echo -e "${RED}[✗] Unsupported package manager${NC}"
        exit 1
    fi
fi

echo -e "${CYAN}[*] Using package manager: $PKG_MANAGER${NC}"

# Update packages
echo -e "${YELLOW}[*] Updating packages...${NC}"
eval $PKG_UPDATE

# Install dependencies
echo -e "${YELLOW}[*] Installing dependencies...${NC}"

if [ "$IS_TERMUX" = true ]; then
    $INSTALL_CMD rust git
    
    # Optional: Install Tor for anonymity
    echo -e "${YELLOW}[*] Installing Tor (optional but recommended)...${NC}"
    $INSTALL_CMD tor || echo -e "${YELLOW}[!] Tor installation failed, continuing without Tor${NC}"
else
    # Linux
    $INSTALL_CMD build-essential git curl || true
    
    # Install Rust if not present
    if ! command -v rustc &> /dev/null; then
        echo -e "${YELLOW}[*] Installing Rust...${NC}"
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        source "$HOME/.cargo/env"
    fi
    
    # Install Tor
    $INSTALL_CMD tor || echo -e "${YELLOW}[!] Tor installation failed${NC}"
fi

# Verify Rust installation
echo -e "${CYAN}[*] Verifying Rust installation...${NC}"
if command -v rustc &> /dev/null; then
    RUST_VERSION=$(rustc --version)
    echo -e "${GREEN}[✓] $RUST_VERSION${NC}"
else
    echo -e "${RED}[✗] Rust not found. Please install manually.${NC}"
    exit 1
fi

# Get script directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Check if we're in the rust_bot directory or need to clone
if [ -f "$SCRIPT_DIR/Cargo.toml" ]; then
    PROJECT_DIR="$SCRIPT_DIR"
    echo -e "${GREEN}[✓] Project found at $PROJECT_DIR${NC}"
else
    echo -e "${YELLOW}[*] Project not found in current directory${NC}"
    
    # Create project directory
    PROJECT_DIR="$HOME/icecold"
    mkdir -p "$PROJECT_DIR"
    
    echo -e "${YELLOW}[*] Please copy the project files to $PROJECT_DIR${NC}"
    echo -e "${YELLOW}    Or run this script from the project directory${NC}"
    exit 1
fi

cd "$PROJECT_DIR"

# Build the project
echo -e "${YELLOW}[*] Building ICE COLD (this may take 5-20 minutes on Termux)...${NC}"
echo -e "${CYAN}    (Go grab a coffee ☕)${NC}"

if [ "$IS_TERMUX" = true ]; then
    # Termux-specific optimizations
    export CARGO_BUILD_JOBS=2  # Limit jobs to prevent OOM on low-end devices
    cargo build --release 2>&1 | tail -20
else
    cargo build --release 2>&1 | tail -20
fi

if [ $? -eq 0 ]; then
    echo -e "${GREEN}[✓] Build successful!${NC}"
else
    echo -e "${RED}[✗] Build failed. Check errors above.${NC}"
    exit 1
fi

# Create symlink
echo -e "${YELLOW}[*] Creating command symlink...${NC}"

if [ "$IS_TERMUX" = true ]; then
    BIN_DIR="$PREFIX/bin"
else
    BIN_DIR="$HOME/.local/bin"
    mkdir -p "$BIN_DIR"
fi

ln -sf "$PROJECT_DIR/target/release/icecold" "$BIN_DIR/icecold"
echo -e "${GREEN}[✓] Created symlink: $BIN_DIR/icecold${NC}"

# Add to PATH if needed (for non-Termux)
if [ "$IS_TERMUX" = false ]; then
    if [[ ":$PATH:" != *":$BIN_DIR:"* ]]; then
        echo 'export PATH="$HOME/.local/bin:$PATH"' >> "$HOME/.bashrc"
        echo -e "${YELLOW}[*] Added $BIN_DIR to PATH in .bashrc${NC}"
    fi
fi

# Create config directory
CONFIG_DIR="$HOME/.config/icecold"
mkdir -p "$CONFIG_DIR"

# Copy example config if it exists
if [ -f "$PROJECT_DIR/config.example.toml" ]; then
    cp "$PROJECT_DIR/config.example.toml" "$CONFIG_DIR/config.toml"
    echo -e "${GREEN}[✓] Config file created at $CONFIG_DIR/config.toml${NC}"
fi

# Create data directory
DATA_DIR="$HOME/.icecold"
mkdir -p "$DATA_DIR"

echo ""
echo -e "${MAGENTA}╔═══════════════════════════════════════════════════════════╗${NC}"
echo -e "${MAGENTA}║                 INSTALLATION COMPLETE! 🎉                  ║${NC}"
echo -e "${MAGENTA}╚═══════════════════════════════════════════════════════════╝${NC}"
echo ""
echo -e "${GREEN}Usage:${NC}"
echo ""
echo -e "  ${CYAN}icecold --help${NC}              Show all options"
echo -e "  ${CYAN}icecold --url \"URL\"${NC}         Start with specific URL"
echo -e "  ${CYAN}icecold --workers 30${NC}        Use 30 workers (default: 50)"
echo -e "  ${CYAN}icecold --no-tor${NC}            Run without Tor"
echo ""
echo -e "${YELLOW}Interactive mode:${NC}"
echo -e "  Run ${CYAN}icecold${NC} and paste your HTML/iframe code"
echo ""
echo -e "${YELLOW}Config file:${NC} $CONFIG_DIR/config.toml"
echo -e "${YELLOW}Data directory:${NC} $DATA_DIR"
echo ""

if [ "$IS_TERMUX" = true ]; then
    echo -e "${YELLOW}[TIP] For best performance on Termux:${NC}"
    echo -e "  - Keep screen on or use ${CYAN}termux-wake-lock${NC}"
    echo -e "  - Use ${CYAN}--workers 30${NC} on low-end devices"
    echo -e "  - Run in background: ${CYAN}nohup icecold --url \"URL\" &${NC}"
    echo ""
fi

echo -e "${GREEN}Ready! Run: icecold --help${NC}"
