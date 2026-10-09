<p align="center">
  <img src="assets/icon.png" width="160" alt="今天用啥">
</p>

<h1 align="center">今天用啥</h1>

<p align="center">轻量的 Windows 电脑使用记录 + AI 日报小工具 · 托盘常驻 · 本地优先</p>

---

「今天用啥」默默待在系统托盘里，记录你每天在电脑上用了哪些应用、逛了哪些网站、看了哪些视频，并聚合成一张便当盒（Bento）风格的仪表盘。可选接入你**自己**的大模型，让它用俏皮的口吻给你写一段当天小结。

> 所有数据只存在你本机，浏览记录永不外发；只有在你**主动开启 AI** 时，才会把**脱敏后的聚合统计**（时长、打码域名）发给你**自己配置**的模型端点。

## ✨ 特性

- 🧃 便当盒仪表盘：有效时长、应用时长（带图标柱状图）、Top 网站 / 视频，数字滚动 + 平滑过渡动画
- 📅 今日 / 近 7 天 / 近 30 天 / 全部 多视图切换
- 🤖 可选 AI 日报：接你自己的 Anthropic / OpenAI 兼容端点，生成一段有情绪的当日小结（支持彩色 emoji）
- 🖥️ 托盘常驻、开机自启（可关）、单实例互斥
- 😴 自动识别空闲与系统休眠，睡眠时间不计入时长
- 🪶 极致轻量（见下）

## 🪶 轻量 & 克制

| 指标 | 实测 |
|---|---|
| 空闲 CPU | < 1%（约 0.02%） |
| 内存 | ~12MB 工作集 |
| 体积 | 单个 exe ~12MB |

坚决不做：❌ 截屏 / 录屏　❌ 键盘记录　❌ OCR　❌ Electron。只靠 Win32 轮询前台窗口 + 系统媒体会话，SQLite 落盘。

## 🔒 隐私

- 全部活动数据存在本机 `%LOCALAPPDATA%\ActivityTracker\data.db`（SQLite），**不上传任何服务器**。
- 浏览网址 / 标题默认留在本地；开启 AI 时只发送**脱敏聚合**（域名打码、可选隐藏视频标题），从不发送原始浏览历史。
- API Key 用 Windows DPAPI 加密存在本地 `key.bin`，不进配置文件、不进仓库。

## 🚀 下载使用

1. 到 [Releases](../../releases) 下载 `今天用啥.exe`。
2. 双击运行——托盘出现图标即在后台记录。
3. **右键托盘图标 → 查看今日统计**，打开仪表盘。

开箱即用，记录功能无需任何配置。AI 小结是可选项（见下）。

## 🤖 开启 AI 日报（可选）

> 这个 exe **不含任何人的 API Key**——key 只存在你本机。别人下载到的就是「不带 key」的版本，AI 默认关闭、需你填自己的。

两步：

**1) 存入你自己的 API Key**（命令行，key 不会出现在进程列表里）：
```powershell
echo 你的APIKEY | .\今天用啥.exe set-key
```

**2) 新建配置** `%APPDATA%\ActivityTracker\config.toml`：
```toml
ai_enabled   = true
ai_api_style = "anthropic"            # 或 "openai"
ai_base_url  = "https://你的端点/v1"  # 填到 /v1；Anthropic 兼容走 /messages，OpenAI 兼容走 /chat/completions
ai_model     = "claude-..."           # 你的端点支持的模型名
```
改完退出托盘再重新打开生效。之后打开仪表盘、当天数据有更新时会自动生成小结。

没配置 AI 完全不影响记录，只是 AI 卡片为空。

## 🛠️ 从源码构建

需要 Rust（Windows，MSVC 或 GNU 工具链均可）：
```bash
cargo build --release
# 产物在 target/release/activity-tracker.exe
```

技术栈：Rust + [windows-rs](https://github.com/microsoft/windows-rs) + [Slint](https://slint.dev)（软件渲染器）+ SQLite（rusqlite）。

---

<p align="center"><sub>个人使用的小工具 · 数据始终在你自己手里</sub></p>

