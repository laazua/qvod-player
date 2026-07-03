#!/usr/bin/env bash
# ═══════════════════════════════════════════════════════════
# QVOD — 跨平台打包脚本
#
# 用法:
#   package.sh [options] <command>
#
# 命令:
#   server                    打包 Linux 服务器版 (qvs-server + qvs-cli)
#   gui-linux   [--server-url URL] 打包 Linux GUI 版 (qvs-gui)
#   gui-windows [--server-url URL] 打包 Windows GUI 版
#   gui-macos   [--server-url URL] 打包 macOS GUI 版
#   all         [--server-url URL] 打包全部
#
# 选项:
#   --server-url URL  设置 GUI 连接的服务端地址 (默认: 空=本地模式)
#   --build-only      仅构建，不打包
#   --package-only    仅打包（需已构建）
#   --dist-dir DIR    输出目录 (默认: dist)
#   --help            显示帮助
#
# 示例:
#   package.sh server                         # 打包 Linux 服务器
#   package.sh gui-linux --server-url http://192.168.1.100:8621
#   package.sh gui-windows --server-url http://example.com:8621
#   package.sh all --server-url http://server:8621
# ═══════════════════════════════════════════════════════════
set -euo pipefail

PKG_NAME="qvs"
VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*= "//;s/"//')
GIT_HASH=$(git log --pretty=format:'%h' -n 1 2>/dev/null || echo "unknown")
ARCH=$(uname -m | sed 's/x86_64/amd64/;s/aarch64/arm64/')
DIST_DIR="dist"
DATE=$(date +%Y%m%d)
SERVER_URL=""
BUILD=true
PACKAGE=true
COMMAND=""

cd "$(dirname "$0")/.."
echo "=== QVOD Packager v${VERSION} (${GIT_HASH}) ==="

# ── Colors ──
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'; NC='\033[0m'
info()  { echo -e "${GREEN}[✓]${NC} $1"; }
warn()  { echo -e "${YELLOW}[!]${NC} $1"; }
error() { echo -e "${RED}[✗]${NC} $1"; exit 1; }
step()  { echo -e "\n${CYAN}━━━ $1 ━━━${NC}"; }

usage() {
    cat <<EOF
用法: $0 [options] <command>

命令:
  server                    打包 Linux 服务器版
  gui-linux   [--server-url URL]  Linux GUI 版
  gui-windows [--server-url URL]  Windows GUI 版
  gui-macos   [--server-url URL]  macOS GUI 版
  all         [--server-url URL]  全部

选项:
  --server-url URL  设置 GUI 连接的服务端地址
  --build-only      仅构建
  --package-only    仅打包
  --dist-dir DIR    输出目录 (默认: dist)
  --help            显示帮助

示例:
  $0 server
  $0 gui-linux --server-url http://192.168.1.100:8621
  $0 all --server-url http://server:8621
EOF
    exit 0
}

# ============================================================
# 参数解析
# ============================================================
while [[ $# -gt 0 ]]; do
    case "$1" in
        server|gui-linux|gui-windows|gui-macos|all)
            COMMAND="$1"
            shift
            ;;
        --server-url)
            SERVER_URL="$2"
            shift 2
            ;;
        --build-only)    BUILD=true;   PACKAGE=false ;;
        --package-only)  BUILD=false;  PACKAGE=true  ;;
        --dist-dir)      DIST_DIR="$2"; shift        ;;
        --help|-h)       usage                       ;;
        *)               error "未知选项/命令: $1"   ;;
    esac
done

[ -z "${COMMAND}" ] && usage
mkdir -p "${DIST_DIR}"

# ============================================================
# 构建函数
# ============================================================
build_linux() {
    step "构建 Linux 版本"
    info "cargo build --release --workspace"
    cargo build --release --workspace 2>&1 | tail -3
    for bin in qvs-cli qvs-server qvs-gui; do
        [ -f "target/release/${bin}" ] && info "  ${bin}: $(ls -lh target/release/${bin} | awk '{print $5}')"
    done
}

build_windows_gui() {
    step "构建 Windows GUI (交叉编译)"
    command -v x86_64-w64-mingw32-gcc >/dev/null || \
        error "MinGW 未安装: sudo apt install mingw-w64"
    rustup target list --installed | grep -q x86_64-pc-windows-gnu || \
        rustup target add x86_64-pc-windows-gnu

    if [ -n "${SERVER_URL}" ]; then
        info "QVS_SERVER_URL=${SERVER_URL}"
        QVS_SERVER_URL="${SERVER_URL}" cargo build --release \
            --target x86_64-pc-windows-gnu -p qvs-gui 2>&1 | tail -3
    else
        cargo build --release --target x86_64-pc-windows-gnu -p qvs-gui 2>&1 | tail -3
    fi
    [ -f "target/x86_64-pc-windows-gnu/release/qvs-gui.exe" ] && \
        info "  qvs-gui.exe: $(ls -lh target/x86_64-pc-windows-gnu/release/qvs-gui.exe | awk '{print $5}')"
}

build_linux_gui() {
    step "构建 Linux GUI"
    if [ -n "${SERVER_URL}" ]; then
        info "QVS_SERVER_URL=${SERVER_URL}"
        QVS_SERVER_URL="${SERVER_URL}" cargo build --release -p qvs-gui 2>&1 | tail -3
    else
        cargo build --release -p qvs-gui 2>&1 | tail -3
    fi
    [ -f "target/release/qvs-gui" ] && info "  qvs-gui: $(ls -lh target/release/qvs-gui | awk '{print $5}')"
}

build_macos_gui() {
    step "构建 macOS GUI (交叉编译)"
    rustup target list --installed | grep -q x86_64-apple-darwin || \
        rustup target add x86_64-apple-darwin
    # Check for osxcross
    command -v o64-clang >/dev/null || warn "osxcross not found, trying native build..."

    if [ -n "${SERVER_URL}" ]; then
        info "QVS_SERVER_URL=${SERVER_URL}"
        QVS_SERVER_URL="${SERVER_URL}" cargo build --release \
            --target x86_64-apple-darwin -p qvs-gui 2>&1 | tail -3
    else
        cargo build --release --target x86_64-apple-darwin -p qvs-gui 2>&1 | tail -3
    fi
    [ -f "target/x86_64-apple-darwin/release/qvs-gui" ] && \
        info "  qvs-gui: $(ls -lh target/x86_64-apple-darwin/release/qvs-gui | awk '{print $5}')"
}

# ============================================================
# 打包函数
# ============================================================

# --- 服务器包 (Linux only: qvs-server + qvs-cli) ---
package_server() {
    step "打包 Linux 服务器版本"

    local dir="${DIST_DIR}/${PKG_NAME}-server-${VERSION}-linux-${ARCH}"
    rm -rf "${dir}"
    mkdir -p "${dir}/bin" "${dir}/config" "${dir}/systemd" "${dir}/scripts"

    cp target/release/qvs-server "${dir}/bin/"
    cp target/release/qvs-cli   "${dir}/bin/"

    # 配置
    cat > "${dir}/config/config.toml" << 'CONF'
[network]
listen_port = 8621
udp_port = 8622
max_connections = 200
enable_dht = true
enable_tracker = true
enable_http_fallback = true
enable_port_forwarding = true

[buffer]
capacity_mb = 256
adaptive = true
max_pipeline_depth = 10

[cache]
max_size_gb = 20
file_format = "sparse"

[tracker]
urls = ["udp://tracker.opentrackr.org:1337/announce"]

[dht]
seed_nodes = ["router.bittorrent.com:6881", "dht.transmissionbt.com:6881"]

[general]
log_level = "info"
CONF

    # systemd 服务
    cat > "${dir}/systemd/qvs-server.service" << 'SVC'
[Unit]
Description=QVOD P2SP Streaming Server
After=network.target network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=/usr/local/bin/qvs-server --config /etc/qvs/config.toml
ExecReload=/bin/kill -HUP $MAINPID
Restart=on-failure
RestartSec=5
LimitNOFILE=65536
MemoryMax=1G

[Install]
WantedBy=multi-user.target
SVC

    # 启动/停止脚本
    cat > "${dir}/scripts/start-server.sh" << 'START'
#!/usr/bin/env bash
exec /usr/local/bin/qvs-server --config /etc/qvs/config.toml "$@"
START

    cat > "${dir}/scripts/stop-server.sh" << 'STOP'
#!/usr/bin/env bash
pkill qvs-server 2>/dev/null && echo "Server stopped" || echo "Server not running"
STOP
    chmod +x "${dir}/scripts/"*.sh

    # 安装说明
    cat > "${dir}/INSTALL.md" << 'INST'
# QVOD Server — Linux 服务器安装

## 快速安装
sudo cp bin/* /usr/local/bin/
sudo mkdir -p /etc/qvs
sudo cp config/config.toml /etc/qvs/
sudo cp systemd/qvs-server.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable qvs-server
sudo systemctl start qvs-server

## 查看状态
sudo systemctl status qvs-server
sudo journalctl -u qvs-server -f

## 测试连接
qvs-cli status
INST

    # RELEASE
    cat > "${dir}/RELEASE" << REL
QVOD Server v${VERSION}
Build: ${DATE} | Git: ${GIT_HASH} | Arch: ${ARCH}
Type: Server (qvs-server + qvs-cli)
REL

    # 打包
    mkdir -p "${DIST_DIR}/packages"
    cd "${DIST_DIR}"
    tar czf "packages/${PKG_NAME}-server-${VERSION}-linux-${ARCH}.tar.gz" \
        "${PKG_NAME}-server-${VERSION}-linux-${ARCH}/"
    cd ..
    info "服务器包: packages/${PKG_NAME}-server-${VERSION}-linux-${ARCH}.tar.gz"
    cd "${DIST_DIR}/packages"
    sha256sum "${PKG_NAME}-server-${VERSION}-linux-${ARCH}.tar.gz" > \
        "${PKG_NAME}-server-${VERSION}-linux-${ARCH}.sha256"
    cd ../..

    local f="${DIST_DIR}/packages/${PKG_NAME}-server-${VERSION}-linux-${ARCH}.tar.gz"
    info "服务器包: $(ls -lh "${f}" | awk '{print $5}')  | 包含 qvs-server + qvs-cli"
}

# --- GUI 包 (Linux) ---
package_gui_linux() {
    local suffix="gui-linux-${ARCH}"
    [ -n "${SERVER_URL}" ] && suffix="${suffix}-server"
    local dir="${DIST_DIR}/${PKG_NAME}-${suffix}-${VERSION}"
    rm -rf "${dir}"
    mkdir -p "${dir}/bin" "${dir}/config" "${dir}/share/applications" \
             "${dir}/share/icons/hicolor/scalable/apps" "${dir}/scripts"

    cp target/release/qvs-gui "${dir}/bin/"
    ln -sf qvs-gui "${dir}/bin/qvs"

    # Icon — use the high-res PNG directly
    cp qvod.png "${dir}/share/icons/hicolor/scalable/apps/qvs.png"
    cp assets/qvod.ico "${dir}/share/icons/"

    # 配置文件
    if [ -n "${SERVER_URL}" ]; then
        cat > "${dir}/config/settings.toml" << TOML
server_url = "${SERVER_URL}"
[network]
listen_port = 8621
[cache]
max_size_gb = 4
[ui]
theme = "dark"
TOML
    fi

    # 桌面集成
    cp assets/qvs.desktop "${dir}/share/applications/"
    cp assets/qvs-qvod-handler.desktop "${dir}/share/applications/"
    cp assets/icon.svg "${dir}/share/icons/hicolor/scalable/apps/qvs.svg"

    # 启动脚本
    if [ -n "${SERVER_URL}" ]; then
        cat > "${dir}/scripts/start-player.sh" << SCRIPT
#!/usr/bin/env bash
# QVOD Player — 连接到服务器: ${SERVER_URL}
exec "\$(dirname "\$0")/../bin/qvs-gui" --server-url "${SERVER_URL}" "\$@"
SCRIPT
    else
        cat > "${dir}/scripts/start-player.sh" << 'SCRIPT'
#!/usr/bin/env bash
exec "$(dirname "$0")/../bin/qvs-gui" "$@"
SCRIPT
    fi
    chmod +x "${dir}/scripts/start-player.sh"

    cat > "${dir}/RELEASE" << REL
QVOD GUI v${VERSION} — Linux
Build: ${DATE} | Git: ${GIT_HASH}
Server: ${SERVER_URL:-local}
REL

    cd "${DIST_DIR}"
    local arch="qvs-${suffix}-${VERSION}.tar.gz"
    tar czf "packages/${arch}" "${PKG_NAME}-${suffix}-${VERSION}/"
    cd ..
    info "Linux GUI 包: packages/${arch} ($(ls -lh "${DIST_DIR}/packages/${arch}" | awk '{print $5}'))"
    cd "${DIST_DIR}/packages"
    sha256sum "${arch}" > "${arch}.sha256"
    cd ../..
}

# --- GUI 包 (Windows) ---
package_gui_windows() {
    local suffix="gui-windows-x86_64"
    [ -n "${SERVER_URL}" ] && suffix="${suffix}-server"
    local dir="${DIST_DIR}/${PKG_NAME}-${suffix}-${VERSION}"
    rm -rf "${dir}"
    mkdir -p "${dir}/bin" "${dir}/config" "${dir}/scripts"

    cp "target/x86_64-pc-windows-gnu/release/qvs-gui.exe" "${dir}/bin/"

    # Icon files — qvod.ico is embedded in the .exe at compile time via
    # build.rs + icon.rc.  We also bundle copies here so installers and
    # shortcuts can reference them at the filesystem level.
    cp qvod.png "${dir}/bin/${PKG_NAME}.png"
    cp assets/qvod.ico "${dir}/bin/${PKG_NAME}.ico"

    # Configuration (Windows path: %APPDATA%/QVOD Player/settings.toml)
    if [ -n "${SERVER_URL}" ]; then
        cat > "${dir}/config/settings.toml" << TOML
server_url = "${SERVER_URL}"
TOML
    fi

    # ── Start scripts ────────────────────────────────────────
    if [ -n "${SERVER_URL}" ]; then
        cat > "${dir}/scripts/start-player.bat" << BATEOF
@echo off
title QVOD Player
start "" "%~dp0..\bin\qvs-gui.exe" --server-url "${SERVER_URL}"
BATEOF
    else
        cat > "${dir}/scripts/start-player.bat" << 'BATEOF'
@echo off
title QVOD Player
start "" "%~dp0..\bin\qvs-gui.exe"
BATEOF
    fi

    # ── Protocol registration ─────────────────────────────────
    cat > "${dir}/scripts/register.bat" << 'REGBAT'
@echo off
title QVOD Protocol Registration
set "EXE=%~dp0..\bin\qvs-gui.exe"

reg add "HKCR\qvod" /ve /t REG_SZ /d "URL:QVOD Protocol" /f
reg add "HKCR\qvod" /v "URL Protocol" /t REG_SZ /d "" /f
reg add "HKCR\qvod\DefaultIcon" /ve /t REG_SZ /d "\"%~dp0..\bin\qvs.ico\"" /f
reg add "HKCR\qvod\shell\open\command" /ve /t REG_SZ /d "\"%EXE%\" \"%%1\"" /f
reg add "HKCR\.qvs" /ve /t REG_SZ /d "QVSFile" /f
reg add "HKCR\QVSFile\DefaultIcon" /ve /t REG_SZ /d "\"%~dp0..\bin\qvs.ico\"" /f
reg add "HKCR\QVSFile\shell\open\command" /ve /t REG_SZ /d "\"%EXE%\" \"%%1\"" /f

echo qvod:// protocol registered!
echo.
echo You can now:
echo   - Double-click .qvs files to open in QVOD Player
echo   - Click qvod:// links in your browser
pause
REGBAT

    # ── Desktop shortcut creator ──────────────────────────────
    cat > "${dir}/scripts/create-shortcut.bat" << 'SHCUT'
@echo off
title Create QVOD Shortcut
set "ICON=%~dp0..\bin\qvs.ico"
set "EXE=%~dp0..\bin\qvs-gui.exe"

:: Create shortcut on Desktop using VBScript
set "SNAME=QVOD Player"
set "VBS=%TEMP%\mklnk.vbs"
echo Set WshShell = WScript.CreateObject("WScript.Shell") > "%VBS%"
echo Set lnk = WshShell.CreateShortcut(WshShell.SpecialFolders("Desktop") ^& "\" ^& "%SNAME%" ^& ".lnk") >> "%VBS%"
echo lnk.TargetPath = "%EXE%" >> "%VBS%"
echo lnk.IconLocation = "%ICON%, 0" >> "%VBS%"
echo lnk.WorkingDirectory = "%~dp0..\bin" >> "%VBS%"
echo lnk.Description = "QVOD P2SP Player" >> "%VBS%"
echo lnk.Save >> "%VBS%"
cscript //nologo "%VBS%"
del "%VBS%"
echo Desktop shortcut created!
pause
SHCUT

    # ── README ────────────────────────────────────────────────
    cat > "${dir}/README.txt" << README
═══════════════════════════════════════
  QVOD Player v${VERSION} — Windows
═══════════════════════════════════════

快速开始
────────
  1. 打开 bin/qvs-gui.exe
  2. 输入或粘贴 qvod:// 链接开始播放
  3. 支持拖放 .qvs 种子文件到窗口

协议注册
────────
  运行 scripts/register.bat 注册 qvod:// 协议,
  之后浏览器点击 qvod:// 链接会自动打开本播放器。

创建桌面快捷方式
────────────────
  运行 scripts/create-shortcut.bat

配置
────
  配置文件位于: %%APPDATA%%\QVOD Player\settings.toml
  (首次启动后自动生成)

鸣谢
────
  Built with Rust + egui
  Server: ${SERVER_URL:-本地模式 (独立运行)}
README

    cd "${DIST_DIR}"
    local arch="${PKG_NAME}-${suffix}-${VERSION}.zip"
    zip -qr "packages/${arch}" "${PKG_NAME}-${suffix}-${VERSION}/"
    cd ..
    info "Windows GUI 包: packages/${arch} ($(ls -lh "${DIST_DIR}/packages/${arch}" | awk '{print $5}'))"
}

# --- GUI 包 (macOS) ---
package_gui_macos() {
    local suffix="gui-macos-x86_64"
    [ -n "${SERVER_URL}" ] && suffix="${suffix}-server"
    local dir="${DIST_DIR}/${PKG_NAME}-${suffix}-${VERSION}"
    rm -rf "${dir}"
    mkdir -p "${dir}/bin"

    cp "target/x86_64-apple-darwin/release/qvs-gui" "${dir}/bin/"
    cp assets/qvod.ico "${dir}/"
    cp qvod.png "${dir}/"

    if [ -n "${SERVER_URL}" ]; then
        cat > "${dir}/start.command" << CMD
#!/usr/bin/env bash
cd "\$(dirname "\$0")"
exec ./bin/qvs-gui --server-url "${SERVER_URL}"
CMD
    else
        cat > "${dir}/start.command" << 'CMD'
#!/usr/bin/env bash
cd "$(dirname "$0")"
exec ./bin/qvs-gui
CMD
    fi
    chmod +x "${dir}/start.command"

    cat > "${dir}/RELEASE" << REL
QVOD GUI v${VERSION} — macOS
Server: ${SERVER_URL:-local}
REL

    cd "${DIST_DIR}"
    local arch="${PKG_NAME}-${suffix}-${VERSION}.tar.gz"
    tar czf "packages/${arch}" "${PKG_NAME}-${suffix}-${VERSION}/"
    cd ..
    info "macOS GUI 包: packages/${arch}"
}

# ============================================================
# 主流程
# ============================================================
case "${COMMAND}" in
    server)
        [ "${BUILD}" = true ] && build_linux
        [ "${PACKAGE}" = true ] && package_server
        ;;
    gui-linux)
        [ "${BUILD}" = true ] && build_linux_gui
        [ "${PACKAGE}" = true ] && package_gui_linux
        ;;
    gui-windows)
        [ "${BUILD}" = true ] && build_windows_gui
        [ "${PACKAGE}" = true ] && package_gui_windows
        ;;
    gui-macos)
        [ "${BUILD}" = true ] && build_macos_gui
        [ "${PACKAGE}" = true ] && package_gui_macos
        ;;
    all)
        [ "${BUILD}" = true ] && { build_linux; build_windows_gui; }
        if [ "${PACKAGE}" = true ]; then
            package_server
            package_gui_linux
            package_gui_windows
        fi
        ;;
esac

# ============================================================
# 汇总
# ============================================================
echo ""
echo "════════════════════════════════════════════"
echo "  QVOD v${VERSION} 打包完成!"
echo "════════════════════════════════════════════"
echo ""
echo "输出目录: ${DIST_DIR}/packages/"
echo ""
ls -lh "${DIST_DIR}/packages/" 2>/dev/null || echo "(无产物)"
