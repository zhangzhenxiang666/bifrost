#!/usr/bin/env bash

set -e

REPO="zhangzhenxiang666/bifrost"
INSTALL_DIR="$HOME/.bifrost/bin"
CONFIG_DIR="$HOME/.bifrost"
CONFIG_FILE="$CONFIG_DIR/config.toml"

# Detect OS and Arch
OS="$(uname -s)"
ARCH="$(uname -m)"
BINARY_SUFFIX=""

detect_platform() {
    if [ "$OS" = "Linux" ]; then
        if [ "$ARCH" = "x86_64" ]; then
            BINARY_SUFFIX="linux-amd64"
        elif [ "$ARCH" = "aarch64" ] || [ "$ARCH" = "arm64" ]; then
            BINARY_SUFFIX="linux-aarch64"
        else
            echo "Unsupported Linux architecture: $ARCH" >&2
            exit 1
        fi
    elif [ "$OS" = "Darwin" ]; then
        if [ "$ARCH" = "x86_64" ]; then
            BINARY_SUFFIX="darwin-amd64"
        elif [ "$ARCH" = "arm64" ]; then
            BINARY_SUFFIX="darwin-aarch64"
        else
            echo "Unsupported macOS architecture: $ARCH" >&2
            exit 1
        fi
    else
        echo "Unsupported OS: $OS" >&2
        exit 1
    fi
}

install_bash() {
    local rc_file="$HOME/.bashrc"
    local init_cmd="export PATH=\"\$HOME/.bifrost/bin:\$PATH\""

    echo "Installing for Bash..."

    # Add to PATH in bashrc
    if ! grep -q ".bifrost/bin" "$rc_file" 2>/dev/null; then
        echo "" >> "$rc_file"
        echo "# bifrost" >> "$rc_file"
        echo "$init_cmd" >> "$rc_file"
        echo "Added PATH configuration to $rc_file"
    else
        echo "PATH configuration already exists in $rc_file"
    fi
}

install_zsh() {
    local rc_file="${ZDOTDIR:-$HOME}/.zshrc"
    local init_cmd="export PATH=\"\$HOME/.bifrost/bin:\$PATH\""

    echo "Installing for Zsh..."

    if ! grep -q ".bifrost/bin" "$rc_file" 2>/dev/null; then
        echo "" >> "$rc_file"
        echo "# bifrost" >> "$rc_file"
        echo "$init_cmd" >> "$rc_file"
        echo "Added PATH configuration to $rc_file"
    else
        echo "PATH configuration already exists in $rc_file"
    fi
}

install_fish() {
    local config_dir="$HOME/.config/fish"
    local init_cmd="set -gx PATH \$HOME/.bifrost/bin \$PATH"

    echo "Installing for Fish..."

    mkdir -p "$config_dir"

    if ! grep -q ".bifrost/bin" "$config_dir/config.fish" 2>/dev/null; then
        echo "" >> "$config_dir/config.fish"
        echo "# bifrost" >> "$config_dir/config.fish"
        echo "$init_cmd" >> "$config_dir/config.fish"
        echo "Added PATH configuration to $config_dir/config.fish"
    else
        echo "PATH configuration already exists in $config_dir/config.fish"
    fi
}

download_and_install() {
    echo "Latest version: $LATEST_TAG"
    ASSET_NAME="bifrost-${LATEST_TAG}-${BINARY_SUFFIX}.tar.gz"
    DOWNLOAD_URL="https://github.com/$REPO/releases/latest/download/$ASSET_NAME"

    echo "Downloading from: $DOWNLOAD_URL"
    TEMP_DIR=$(mktemp -d)
    trap "rm -rf $TEMP_DIR" EXIT

    if ! curl -L "$DOWNLOAD_URL" -o "$TEMP_DIR/$ASSET_NAME" 2>/dev/null; then
        echo "Error: Failed to download asset" >&2
        exit 1
    fi

    echo "Extracting..."
    tar -xzf "$TEMP_DIR/$ASSET_NAME" -C "$TEMP_DIR"

    # Find bifrost binary
    BIFROST_BIN=$(find "$TEMP_DIR" -type f -name "bifrost-$BINARY_SUFFIX" | head -n 1)
    BIFROST_SERVER_BIN=$(find "$TEMP_DIR" -type f -name "bifrost-server-$BINARY_SUFFIX" | head -n 1)

    if [ -z "$BIFROST_BIN" ]; then
        echo "Error: Could not find bifrost binary in archive" >&2
        exit 1
    fi

    if [ -z "$BIFROST_SERVER_BIN" ]; then
        echo "Error: Could not find bifrost-server binary in archive" >&2
        exit 1
    fi

    # Install binaries
    mkdir -p "$INSTALL_DIR"
    mv "$BIFROST_BIN" "$INSTALL_DIR/bifrost"
    mv "$BIFROST_SERVER_BIN" "$INSTALL_DIR/bifrost-server"
    chmod +x "$INSTALL_DIR/bifrost"
    chmod +x "$INSTALL_DIR/bifrost-server"

    echo "Installed binaries to $INSTALL_DIR"
}

create_default_config() {
    if [ -f "$CONFIG_FILE" ]; then
        echo "Config file already exists at $CONFIG_FILE"
        return
    fi

    echo "Creating default config at $CONFIG_FILE..."
    mkdir -p "$CONFIG_DIR"

    cat > "$CONFIG_FILE" << 'EOF'
# =============================================================================
# Bifrost Server Configuration
# =============================================================================

[server]
port = 5564
timeout_secs = 600
max_retries = 5
EOF

    echo "Created default config at $CONFIG_FILE"
}

main() {
    # Parse arguments
    SHELL_TYPE=""
    if [ $# -gt 0 ]; then
        case "$1" in
            bash) SHELL_TYPE="bash" ;;
            zsh) SHELL_TYPE="zsh" ;;
            fish) SHELL_TYPE="fish" ;;
            *) echo "Usage: $0 [bash|zsh|fish]" >&2; exit 1 ;;
        esac
    else
        # Auto-detect shell
        DETECTED_SHELL="${SHELL:-""}"
        if [ -z "$DETECTED_SHELL" ]; then
            if [ -n "$BASH_VERSION" ]; then
                DETECTED_SHELL="bash"
            elif [ -n "$ZSH_VERSION" ]; then
                DETECTED_SHELL="zsh"
            elif command -v fish &>/dev/null; then
                DETECTED_SHELL="fish"
            fi
        fi
        SHELL_TYPE=$(basename "$DETECTED_SHELL")
    fi

    echo "Detected OS: $OS, Arch: $ARCH"
    echo "Installing for shell: $SHELL_TYPE"

    detect_platform

    # Get latest release tag
    echo "Fetching latest version..."
    LATEST_URL=$(curl -Ls -o /dev/null -w "%{url_effective}" "https://github.com/$REPO/releases/latest" 2>/dev/null)
    LATEST_TAG=$(basename "$LATEST_URL")

    if [ -z "$LATEST_TAG" ]; then
        echo "Error: Could not determine latest version" >&2
        exit 1
    fi

    download_and_install
    create_default_config

    # Configure shell
    case "$SHELL_TYPE" in
        bash) install_bash ;;
        zsh) install_zsh ;;
        fish) install_fish ;;
    esac

    echo ""
    echo "Installation complete!"
    echo ""
    echo "Usage:"
    echo "  - bifrost: Run 'bifrost' from anywhere (already in PATH)"
    echo "  - bifrost-server: Use full path '$INSTALL_DIR/bifrost-server'"
    echo ""
    echo "Please restart your shell or source your config file to use 'bifrost'."
}

main "$@"
