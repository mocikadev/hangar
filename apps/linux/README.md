# Hangar Linux 原生版

开发中，未公开发布。GNOME 风格的 GTK4/Libadwaita 应用，通过 Rust 绑定直接调用 hangar-core。

## 开发

当前构建/验收宿主：Ubuntu 26.04 x86_64。API 需要 GTK >=4.14、Libadwaita >=1.5；更旧发行版、其他 CPU 和 KDE 未验证。Rust 工具链由仓库固定。

```sh
sudo apt install build-essential pkg-config libgtk-4-dev libadwaita-1-dev libglib2.0-dev-bin
cargo build --locked -p hangar-linux
scripts/check-linux.sh
# 可选：当前图形会话的无凭据自动冒烟（需 python3-gi / gir1.2-atspi-2.0）
scripts/test-linux-desktop.py
# 可选：可重复的 GTK 布局与空配置对话框测试（需 xvfb）
GDK_BACKEND=x11 xvfb-run -a scripts/test-linux-layout.sh
```

日常运行 `cargo run -p hangar-linux` 会访问共享真实账号库并自动刷新；开发验证使用 `scripts/run-linux-qa.sh` 的空隔离配置（保留桌面 portal 与主题，使用独立 QA 应用身份，不能复现真实账号成功请求）。不要把真实 token 写入命令、日志或测试数据。

## 操作

- Ctrl+N 添加；Ctrl+R 强制刷新；Ctrl+Shift+D 自检；Ctrl+Q 完全退出；Ctrl+W 关闭窗口。
- 账号卡片显示套餐、周额度、重置卡及当前/推荐/重新登录状态，不显示 AT/RT，更多菜单提供重新登录和删除。新增/重登不自动切换。
- 登录对话框点击“在浏览器中打开”；自动回调不可用时可粘贴完整回调地址。
- 系统深浅色由 Libadwaita 跟随桌面设置；不单独安装或配置皮肤。
- 桌面支持 StatusNotifierItem 时显示状态栏图标：账号额度、快捷切换、刷新、显示窗口、退出。展开菜单不会发起额度请求。
- 状态栏已注册时，关闭窗口会隐藏并继续运行；用状态栏“显示 Hangar”或再次启动恢复。入口丢失时自动恢复窗口，避免后台进程无法找回。
- 状态栏不可用时，关闭窗口会等待在途操作完成并退出；最小化仍可保留运行。菜单“退出”或 Ctrl+Q 始终完全退出，不设置开机启动。
- 登录/重登成功会立即刷新额度，重登窗口显示目标邮箱。系统唤醒后检查到期缓存；忙碌时延后执行一次。
- 当前账号不可删除；未知/过期/失败数据不会伪装成实时额度或用于推荐。

## 构建 deb

仅提供个人使用的 deb，不提供 AppImage/tar.gz。包名与命令均为 `hangar-desktop`，不覆盖 CLI 的 `hangar`。保持系统深浅主题和 GTK 的显示后端选择，打包不设置 `GTK_THEME` 或 `GDK_BACKEND`。

```sh
sudo apt install build-essential pkg-config libgtk-4-dev libadwaita-1-dev libglib2.0-dev-bin dpkg-dev binutils desktop-file-utils
scripts/check-linux.sh
scripts/package-linux.py
```

脚本显式构建本机 GNU Linux target 的 Release，从 Cargo JSON 获取可执行文件，不依赖固定 target 目录。输出 `dist/linux/<build-id>/`：

- `hangar-desktop_0.7.0-1_amd64.deb`（实际版本取 Cargo；arm64 命名已映射但未验证）
- `manifest.json`：系统、target、构建版本、动态库依赖、Git/dirty、包大小与 SHA
- `SHA256SUMS.txt`：校验 deb 与 manifest

`dpkg-shlibdeps` 根据实际链接生成依赖。Ubuntu 26.04 构建的包只按该宿主验证，不能当成 Ubuntu 24.04 通用包；CI 使用 Ubuntu 24.04 单独构建，是否运行通过以 CI 证据为准。包未签名，SHA 校验用于检查字节完整性。本地打包不会发布或安装程序。

```sh
# 将路径换成本次脚本输出的目录
scripts/verify-linux-package.py dist/linux/<build-id>
# 可选：从 deb 解包后进行空隔离配置的图形启动/单实例/退出验证
GDK_BACKEND=wayland scripts/verify-linux-package.py dist/linux/<build-id> --smoke
```

验收脚本只在临时目录解包、使用独立 dpkg 数据库登记/重复解包/移除并检查 CLI 与账号目录保留，不绕过本机 apt 依赖或安装到系统；这与完整系统安装验收分别记录。图形验证需要 python3-gi、gir1.2-atspi-2.0、dbus-x11、at-spi2-core、fuse3。

## 安装、升级与卸载

先从菜单“退出”当前开发预览，避免单实例继续显示旧进程。在本次产物目录运行：

```sh
sha256sum -c SHA256SUMS.txt
sudo apt install ./hangar-desktop_0.7.0-1_amd64.deb
hangar-desktop
```

也可以从应用启动器打开 Hangar。若系统已有更早手动安装到 `~/.local/bin` 的版本，先用 `type -a hangar-desktop` 检查入口；deb 不会删除用户目录中的手动安装。更新版本使用同样的 apt 安装命令；相同版本重新构建使用 `sudo apt install --reinstall ./hangar-desktop_0.7.0-1_amd64.deb`。本机自行构建时保留旧 deb，可用 `sudo apt install --allow-downgrades ./旧版本.deb` 回退；二进制回退不能恢复服务端已轮换的凭据。

```sh
sudo apt remove hangar-desktop
```

包只包含程序、desktop 文件、图标和说明/许可证，不含安装脚本，不自动启动应用、设开机启动或写 HOME；卸载保留 `~/.hangar`、Codex 数据与 CLI。

## 桌面集成无凭据验收

构建 Debug 后可运行 `/usr/bin/python3 scripts/test-linux-integration.py`（私有状态栏宿主，包含失联/重连）与 `--real-session`（本机真实状态栏、独立空账号实例）。`/usr/bin/python3 scripts/test-linux-theme.py` 用私有 Settings portal 验证主题信号，`--real-session` 只读比较桌面偏好。需要系统 python3-gi、gir1.2-atspi-2.0、gir1.2-adw-1 和图形会话；测试不会修改系统主题。
