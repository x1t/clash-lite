<div align="center">

# 🐱 Clash Lite

**一个只有 1.6 MB 的 Windows 托盘客户端，给 [mihomo](https://github.com/MetaCubeX/mihomo) 内核用**

不带 WebView，不常驻窗口，不轮询。右键托盘就能用。

[![Release](https://img.shields.io/github/v/release/x1t/clash-lite?style=flat-square&color=4CAF50)](https://github.com/x1t/clash-lite/releases/latest)
[![Build](https://img.shields.io/github/actions/workflow/status/x1t/clash-lite/build.yml?style=flat-square&label=build)](https://github.com/x1t/clash-lite/actions/workflows/build.yml)
![Platform](https://img.shields.io/badge/platform-Windows%2010%20%7C%2011-0078D6?style=flat-square&logo=windows)
![Rust](https://img.shields.io/badge/made%20with-Rust-B7410E?style=flat-square&logo=rust)

</div>

---

## ✨ 为什么做这个

Clash Verge 很好用，但它的界面跑在 WebView2 上，光界面就要几百 MB 内存。
Clash Lite 只保留**托盘菜单**，真正干活的还是同一个 mihomo 内核：

| 作者机器实测 | 工作集 | 私有内存 |
|---|---:|---:|
| Clash Verge：主程序 + WebView2 + 服务 | ~605 MB | ~339 MB |
| **Clash Lite 托盘程序** | **~12 MB** | **~2 MB** |

> mihomo 内核本身的内存两者一样，和订阅大小有关，作者的订阅约 55 MB。数字只是一台机器上的测量结果，仅供参考。

## 📦 功能

- 🔗 **订阅**：从剪贴板一键添加，支持多订阅切换、手动更新和定时自动更新，显示已用流量和到期时间
- 🌐 **节点**：右键时实时读取策略组，点击切换节点，按组测速并显示延迟
- 🧭 **模式**：规则 / 全局 / 直连
- 🛡️ **TUN 模式**：接管所有程序的流量
- 🖥️ **系统代理**：一键开关，退出时自动关闭，不会留下连不上的代理
- 🚀 **开机自启**：用最高权限计划任务启动，开机不弹 UAC
- 🤫 **静默启动**：默认只在托盘运行；关掉后每次启动会自动打开面板
- 📊 **Web 面板**：内置 [Yacd](https://github.com/MetaCubeX/Yacd-meta)，在浏览器里打开并自动登录
- ⬆️ **更新 mihomo**：一键升级到官方最新版，不影响正在用的网络
- 📥 **零配置**：只需一个 exe，第一次运行自动下载 mihomo 和面板

## 🚀 快速开始

1. 从 [Releases](https://github.com/x1t/clash-lite/releases/latest) 下载 `clash-lite.exe`，放进一个**单独的空文件夹**
2. **双击运行**。程序会自动申请管理员权限（TUN 需要），弹出 UAC 时点"是"
3. 第一次运行会自动下载 mihomo 和面板，期间托盘图标是灰色 ⚪
4. 复制订阅链接，右键托盘 → **订阅 → 从剪贴板添加订阅**，图标变绿 🟢
5. 右键勾选 **开机自启**，以后就不用管了

### 托盘图标

| 图标 | 含义 |
|:---:|---|
| ⚪ 灰 | 内核未运行（还没添加订阅，或正在下载组件） |
| 🟢 绿 | 运行中 |
| 🔵 蓝 | 运行中，TUN 已开启 |

### 右键菜单

```
订阅 ▸            ✔ 机场A  (12.0G/200.0G · 2026-12-01)
                    从剪贴板添加订阅 / 更新当前订阅 / 删除订阅 ▸
─────────
PROXY  [hk] ▸     测速 / ✔ hk  617ms / sg  2116ms …
youtube [cc] ▸    …
─────────
模式 ▸            ✔ 规则 / 全局 / 直连
✔ TUN 模式
  系统代理
─────────
打开面板  查看日志  更新 mihomo
✔ 静默启动  ✔ 开机自启
退出
```

## 📁 文件都放在哪

所有文件都在 exe 同目录，**不写 AppData**，整个文件夹拷走就能用：

```
clash-lite/
├─ clash-lite.exe   托盘程序
├─ mihomo.exe       内核（自动下载）
├─ ui/              Yacd 面板（自动下载）
├─ state.json       设置、订阅列表、控制接口密钥
├─ profiles/        订阅原文
├─ data/            mihomo 工作目录（规则库缓存等）
└─ mihomo.log       内核日志（超过 5 MB 自动轮转）
```

> 🔒 `state.json` 和 `profiles/` 里有你的订阅链接和节点信息，**分享文件夹之前请删掉它们**。

## ❓ 常见问题

<details>
<summary><b>为什么要管理员权限？</b></summary>

TUN 需要创建虚拟网卡，只有管理员能做。Clash Lite 用的是 Windows 标准方式（在 exe 里声明需要管理员），由系统弹 UAC。**没有任何绕过 UAC 的手段**。开启开机自启后，每次开机由计划任务直接以管理员身份启动，不弹窗。
</details>

<details>
<summary><b>第一次运行提示"下载组件失败"？</b></summary>

国内网络可能连不上 GitHub。可以先开着别的代理再运行一次，也可以手动从 [mihomo Releases](https://github.com/MetaCubeX/mihomo/releases/latest) 下载 `mihomo-windows-amd64-v2-*.zip`，解压后改名为 `mihomo.exe`，放到程序目录。之后更新 mihomo、更新订阅时，如果直连失败，会自动通过 Clash Lite 自己的代理重试。
</details>

<details>
<summary><b>代理端口是多少？</b></summary>

用订阅里写的 `mixed-port` 或 `port`；订阅里都没写时用 `7897`。控制接口固定是 `127.0.0.1:19097`，密钥在第一次运行时随机生成。
</details>

<details>
<summary><b>能和 Clash Verge 同时运行吗？</b></summary>

可以运行，但两边会抢**系统代理**，开着 TUN 也会互相冲突，建议只留一个。
</details>

<details>
<summary><b>杀毒软件报毒？</b></summary>

新编译的 exe 没有签名，下载量也小，偶尔会被启发式规则误报。代码全部开源，可以自己用 `cargo build --release` 编译。
</details>

## 🛠️ 从源码编译

```powershell
cargo build --release                 # 正式版，需要管理员权限运行
$env:CLASH_LITE_NO_ADMIN=1; cargo build --release   # 不要求管理员的版本，TUN 不可用
cargo test                            # 单元测试
cargo test -- --include-ignored --test-threads=1    # 加上联网的端到端测试（真实下载 mihomo）
```

需要 Rust 1.85+（edition 2024）和 MSVC 工具链。推送 `v*.*.*` 标签后，GitHub Actions 会自动编译并发布 Release。

## 🙏 致谢

- [MetaCubeX/mihomo](https://github.com/MetaCubeX/mihomo)：内核
- [MetaCubeX/Yacd-meta](https://github.com/MetaCubeX/Yacd-meta)：Web 面板
- [clash-verge-rev](https://github.com/clash-verge-rev/clash-verge-rev)：订阅处理和 TUN 的参考实现
- [tauri-apps/tray-icon](https://github.com/tauri-apps/tray-icon)：托盘
