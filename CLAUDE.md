# CLAUDE.md

本文件给在这个仓库里工作的 AI 编码助手看。用户文档见 [README.md](README.md)。

## 项目一句话

Windows 托盘程序，管理 `mihomo.exe` 子进程，通过 REST API（`127.0.0.1:19097`）控制它。**最高目标是省内存**，其次是简单。

## 硬性约束（改代码前先看）

1. **不要加 WebView、GUI 框架或常驻窗口**。只能有托盘菜单，复杂的 UI 交给浏览器里的 Yacd 面板。
2. **不轮询**。菜单在右键时现场生成（`TrayIconEvent` → `menu::build` → `set_menu`）。唯一的后台循环是订阅定时更新线程，它会一直睡到下一个订阅到期，或被 `wake` 通道唤醒。
3. **不要绕过 UAC**。管理员权限只来自 `build.rs` 写进清单的 `requireAdministrator`，开机自启用最高权限计划任务（`autostart.rs`）。曾经用"按需计划任务免 UAC 自启动"的写法，被卡巴斯基报成 `Trojan-PSW`，已经删掉，不要再加回来。
4. **依赖要克制**。HTTP 用 `ureq`（阻塞式，不带 tokio），解压调用系统自带的 `System32\tar.exe`，不用 zip 库。新增 crate 前先问用户。
5. **只支持 Windows**。直接用 `windows-sys` 调 Win32，不做跨平台抽象。
6. **订阅原文不改**（用户选的方案 A）。端口、TUN、日志级别在内核启动后用 `PATCH /configs` 覆盖；控制地址和密钥用启动参数 `-ext-ctl`、`-secret` 覆盖。

## 模块地图

| 文件 | 职责 |
|---|---|
| `main.rs` | 单实例互斥锁、托盘、Win32 消息循环（`GetMessageW` 阻塞）、启动后台线程 |
| `app.rs` | `App` = `Mutex<Inner{state, core, port}>`。所有业务操作：启动/重启、切换/更新/删除订阅（失败回滚）、TUN、系统代理、更新 mihomo |
| `menu.rs` | 生成菜单、处理菜单事件；耗时操作都放进 `task()` 在线程里执行 |
| `core.rs` | 启停 mihomo 子进程，挂到 Job 对象（`KILL_ON_JOB_CLOSE`，托盘被强杀时内核也跟着退出），日志轮转 |
| `api.rs` | mihomo REST 客户端，要带 Bearer 密钥 |
| `subscription.rs` | 下载订阅（UA 固定为 `xctcc`）、解析 `subscription-userinfo` 等响应头、校验内容 |
| `assets.rs` | 第一次运行时下载 mihomo（`amd64-v2`）和 `assets/yacd.zip`；先下载到临时目录，再替换正式文件 |
| `update.rs` | 自我更新：查 GitHub 最新 Release，下载新 exe，把运行中的 exe 改名为 `.old`、新文件就位、重启（`--post-update` 让新进程等旧进程退出后删掉 `.old`）。不做签名校验，只校验 PE 头和大小 |
| `state.rs` | `state.json` 的读写（先写临时文件再改名），`base_dir()` = exe 所在目录 |
| `sysproxy.rs` / `autostart.rs` / `clipboard.rs` / `ui.rs` | 注册表代理设置 / 计划任务 / 剪贴板 / 弹窗、图标、线程消息 |

## 并发规则

- UI 线程（托盘、菜单回调）只能用 `app.try_lock()`，**不能阻塞**。锁被占用时菜单显示"正在处理"。
- 后台操作用 `app.lock()`，做完后调用 `ui::refresh()`，给主线程发 `WM_APP` 消息，让它同步托盘图标和提示文字。
- 耗时的网络下载要在**拿锁之前**做完（参考 `update` 和 `update_mihomo`），持锁期间只做替换文件和重启这类很快的操作。

## 常用命令

```powershell
cargo clippy --all-targets -- -D warnings   # 必须没有任何警告
cargo fmt
cargo test                                  # 单元测试（CI 也跑这个）
cargo test -- --include-ignored --test-threads=1   # 加上端到端测试：真实 mihomo、真实下载、真实剪贴板
$env:CLASH_LITE_NO_ADMIN=1; cargo build --release  # 本地调试用，不要求管理员
```

- 端到端测试（`e2e_tests.rs`）在 `target/debug/deps/` 里运行。测试构建用的控制端口是 **19098**（`core::CTL_PORT`），所以开着正式版程序也能跑测试。
- 端到端测试会改系统剪贴板（测完会恢复）；会从 GitHub 下载 mihomo v1.19.31 和最新版。
- **不要在测试里开关系统代理或 TUN**：用户机器上常常同时开着 Clash Verge，会影响用户正在用的网络。

## 发版

1. 修改 `Cargo.toml` 的 `version`
2. 提交，推到 `main`
3. `git tag -a vX.Y.Z -m vX.Y.Z && git push origin vX.Y.Z`
4. `.github/workflows/build.yml` 在 `windows-latest` 上依次跑 clippy、测试、release 编译，再用 `softprops/action-gh-release@v3` 把**单个** `clash-lite.exe` 发布到 Release

更换面板：把编译好的面板（根目录直接是 `index.html`）重新打包成 `assets/yacd.zip` 并提交。程序从 `https://github.com/x1t/clash-lite/raw/main/assets/yacd.zip` 下载它。面板必须支持 `?hostname=&port=&secret=` 这几个链接参数（Yacd 支持；metacubexd 只读 `#/setup?` 后面的参数，不兼容）。

## 隐私

- 不要提交 `state.json`、`profiles/`、`*.log` 或任何真实订阅链接，测试里只用本地起的 HTTP 服务。
- 提交前 grep 一下，确认 diff 里没有订阅域名或密钥。

## 代码风格

- 函数短小，注释只写代码本身表达不了的约束，不写"这行在干嘛"。
- 给用户看的错误提示用中文，代码和注释用英文。
- 文件换行用 LF（`.gitattributes` 已强制）。
