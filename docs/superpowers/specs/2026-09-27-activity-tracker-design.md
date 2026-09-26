# 轻量级 Windows 活动记录 + AI 总结工具 — 设计规格

- 状态：草案（待审查）
- 日期：2026-09-27
- 技术栈：Rust + windows-rs + Slint + SQLite
- 适用范围：仅记录本人本机设备，本地优先、隐私安全、资源占用极低

---

## 0. 概述

一个常驻系统托盘的 Windows 活动记录工具：以**事件驱动**方式记录"打开了什么应用、具体在干什么、每项持续多久"，按小时/天聚合后调用 AI 生成自然语言总结。**轻量化优先级高于功能完整度**。

**设计标杆**（来自需求参考案例）：
- ActivityWatch —— 分层架构、采集器插件化、本地优先。
- Focusd —— 极简实现、采集/展示分离、~9MB、资源占用极低（轻量化标杆）。
- DayLens —— `GetLastInputInfo` 系统级空闲检测、心跳 + 崩溃恢复清理幻影记录。
- Dayflow —— 理解"在做什么"而非只记窗口标题；AI 总结、Markdown 导出。
- Yolo —— **数值本地确定性计算，AI 只负责叙述**，模型不编造数字。

## 1. 范围与非目标

**做（v1）：**
- 前台应用/进程/窗口标题/时长追踪（事件驱动）。
- 系统级空闲检测（人不在时不计有效活动）。
- Edge 当前标签 URL + 页面标题（UI Automation）。
- 媒体标题/播放状态/进度（SMTC 系统媒体会话）。
- 本地 SQLite 永久存储 + 批量写入 + 按小时/天聚合。
- AI 总结（OpenAI 兼容接口，只发脱敏聚合摘要）。
- 托盘 + 按需显示的 Slint 窗口（Bento Grids 风格）。

**不做（明确非目标）：**
- 不支持 Edge 以外的浏览器。
- 不录屏、不 OCR、不全局键盘钩子、不抓包、不读取视频内容。
- 不使用 Electron / Chromium 内核 / 完整浏览器内核。
- 不自动清理 / 不设过期 / 不自动销毁数据。
- 不上传原始浏览历史或原始记录。

## 2. 技术选型（定案）

| 关注点 | 选择 | 理由 |
|--------|------|------|
| 语言 | Rust | 体积/内存最小，无运行时依赖 |
| 系统 API | `windows`(windows-rs) | Win32（WinEventHook/空闲/进程/DPAPI）+ WinRT（SMTC）+ UI Automation |
| 窗口 UI | Slint | 无 Chromium、软件渲染、内存低、可自定义样式；平时隐藏 ≈ 零成本 |
| 托盘 | `tray-icon` | 轻量托盘图标 + 菜单 |
| 存储 | `rusqlite`（bundled） | SQLite 静态链接，免外部依赖 |
| 序列化 | `serde` / `serde_json` / `toml` | 配置 + AI 载荷 |
| HTTP | `ureq`（+ rustls） | 极小的阻塞式 HTTP，用于 AI 调用 |
| 时间 | `time` | 轻量时间戳/格式化 |
| 通道 | `std::sync::mpsc` 或 `crossbeam-channel` | 采集线程 → 存储线程事件总线 |

**密钥加密**：Windows DPAPI（`CryptProtectData`/`CryptUnprotectData`），绑当前 Windows 账户，不明文落盘。

**发布配置**（守体积）：`opt-level = "z"`、`lto = true`、`codegen-units = 1`、`panic = "abort"`、`strip = true`。预计安装包 5–15MB（< 30MB 硬指标）。

## 3. 总体架构

单进程、多线程、事件驱动。三层：**采集器（Watchers）→ 事件总线/存储 → 展示与 AI**。各采集器互不依赖、可独立开关。

```
┌──────────────────────────── 单进程 (Rust) ────────────────────────────┐
│                                                                        │
│  [采集线程 A]          [媒体线程 B]           [空闲检测]                 │
│  SetWinEventHook       SMTC 会话监听+采样      GetLastInputInfo          │
│  前台切换/标题变化      标题/状态/进度                                    │
│  + Edge UIA 读地址栏                                                    │
│       │                     │                     │                    │
│       └──────────┬──────────┴─────────────────────┘                    │
│                  ▼  ActivityEvent (mpsc 事件总线)                       │
│         [存储线程 C] 会话合并 → 批量缓冲 → SQLite (WAL, 单写连接)         │
│                  ▲ 读连接（只读，WAL 并发读）                            │
│       ┌──────────┴───────────┐                                         │
│  [主线程: 托盘 + Slint 窗口]    [AI 总结: 定时/按需线程]                  │
│  暂停/统计/立即总结/设置/退出    聚合→脱敏→OpenAI兼容接口→存 summaries     │
└────────────────────────────────────────────────────────────────────────┘
```

**线程职责**
- **主线程**：托盘 + Slint 事件循环；窗口懒创建、隐藏即低成本。查询走只读连接。
- **线程 A（采集）**：拥有独立 Win32 消息循环（`SetWinEventHook` 要求），响应 `EVENT_SYSTEM_FOREGROUND` 与 `EVENT_OBJECT_NAMECHANGE`；前台为 Edge 时触发 UIA 读取。
- **线程 B（媒体）**：`GlobalSystemMediaTransportControlsSessionManager` 订阅会话变化事件 + 播放时按间隔采样 `Position`。
- **线程 C（存储）**：唯一写入者；接收事件、合并会话、批量落盘。
- **AI 线程**：定时（默认按天）或按需触发，独立于采集，失败不影响记录。

> 事件循环接线细节（tray-icon 与 Slint winit 后端的桥接、`invoke_from_event_loop`）留待实现计划阶段确定。

## 4. 模块设计

### 4.1 前台窗口采集器（Foreground Watcher）
- `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)` 捕获前台切换；`EVENT_OBJECT_NAMECHANGE` 捕获同窗口内标题变化（如浏览器换标签、IDE 换文件）。
- 拿到 HWND → `GetWindowThreadProcessId` → `OpenProcess` + `QueryFullProcessImageNameW` 取进程完整路径与进程名。
- 取窗口标题 `GetWindowTextW`。
- 兜底：以可配置低频（默认 5s）轮询 `GetForegroundWindow`，捕获漏掉的事件（也用于 UIA/标题的稳态刷新）。
- 输出 `ActivityEvent { ts, app_name, process_path, window_title, hwnd }`。

### 4.2 空闲检测（Idle Detector）
- `GetLastInputInfo` 计算距上次输入的时长；超过阈值（默认 60s）判定为空闲。
- 空闲期间：关闭当前活动会话或标记 `is_idle=1`，**不累计有效时长**（避免"屏幕亮着人不在"的虚假记录，DayLens 同款）。
- 恢复输入后开新会话。

### 4.3 Edge 标签采集器（UI Automation）
- **仅当前台进程为 `msedge.exe` 时触发**（省资源，不常驻 UIA 全树遍历）。
- 用 `IUIAutomation` 定位地址栏（Edit 控件，按 `AutomationId`/`ControlType` 缓存请求定位），读 `ValuePattern.CurrentValue` → 当前 URL。
- 页面标题：优先窗口标题解析（Edge 标题格式 `"<页面标题> - … - Microsoft Edge"`）。
- 视频站点识别：URL/标题启发式匹配 YouTube、B站等，提取视频标题。
- **InPrivate 识别**：窗口标题/UIA 特征（InPrivate 标记）→ 默认不记录，或按配置标记 `is_private=1`。
- **降级**：读不到地址栏（聚焦输入中/控件缺失）时，退化为仅窗口标题；不因此报错。

### 4.4 媒体采集器（SMTC）
- WinRT `GlobalSystemMediaTransportControlsSessionManager::RequestAsync()`（`.get()` 阻塞在专用线程）。
- 遍历会话：`GetPlaybackInfo()` → `PlaybackStatus`（Playing/Paused/Stopped）；`TryGetMediaPropertiesAsync()` → Title/Artist；`GetTimelineProperties()` → `Position` / `EndTime`（进度与总时长）；`SourceAppUserModelId` → 播放器标识（映射到应用名）。
- 订阅 `SessionsChanged` / `MediaPropertiesChanged` / `PlaybackInfoChanged` 事件 + 播放时按间隔采样 `Position`（用于进度）。
- 输出 `MediaEvent { ts, media_title, media_player, media_status, position_sec, duration_sec }`，与前台会话关联。
- **降级**：会话不上报 timeline（部分站点/播放器）→ 仅记录标题 + 状态，进度置空，时长由会话持续时间兜底。

### 4.5 事件总线与会话合并
- 采集线程通过 mpsc 把 `ActivityEvent` / `MediaEvent` 发给存储线程。
- **会话合并（纯函数，可单测）**：维护"当前会话"。当关键字段 `(app_name, window_title, edge_url, media_title, is_private, is_idle)` 与新事件一致且时间间隔在容差内 → 延长当前会话 `end_ts`；任一关键字段变化 → 结算当前会话（写 `end_ts`、`duration_sec`）并开新会话。
- 媒体事件更新当前会话的 `media_*` 字段（进度取最新采样）。

### 4.6 存储层（SQLite）
- `rusqlite` bundled；数据库文件放 `%LOCALAPPDATA%\ActivityTracker\data.db`。
- **WAL 模式**：单写连接（线程 C 独占）+ UI 只读连接并发读，避免锁竞争。
- **批量写入**：事件先入内存缓冲，满足"每 15s（默认）或攒够 200 条"其一即在单事务内 flush；暂停/退出时强制 flush。
- **心跳 + 崩溃恢复（DayLens 思路）**：定期写心跳时间戳；启动时把上次遗留的"未结算会话"（无 `end_ts`）按最后心跳时间封口，清理幻影数据。

### 4.7 聚合查询层
- 提供按小时、按天的聚合：各应用/域名总时长、Top 应用/网站/视频、时间线（会话序列）、有效时长 vs 空闲。
- 数值全部由 SQL 确定性计算（供 UI 展示与 AI 载荷），AI 不参与计算。

### 4.8 AI 总结模块
- 触发：定时（默认按天，可配按小时）+ 托盘"立即生成"。
- 流程：聚合 → **脱敏 + 排除表过滤** → 组装结构化 JSON（时间线块、应用/域名时长分布、主题）→ `ureq` POST 到 OpenAI 兼容 `/chat/completions` → 存自然语言总结到 `summaries` 表。
- **数值本地算好一起发**（Yolo 原则），提示词约束模型只叙述、不编数字。
- 可一键关闭 AI（关闭后仅本地统计）。失败重试有限次，不阻塞采集。
- **只发送脱敏聚合摘要，绝不发送原始记录/完整浏览历史。**

### 4.9 托盘
- `tray-icon`：菜单项 = 暂停/恢复记录、查看今日统计（显示窗口）、立即生成总结、设置、退出。
- 暂停状态图标区分；暂停即停止所有采集线程的写入。

### 4.10 Slint 窗口 UI
- 平时不显示（或创建后隐藏）；托盘"查看统计"时显示，关闭时隐藏/销毁回到低成本态。
- 内容：今日概览（Bento 网格卡片）——总有效时长、应用时长排行、活动时间线、Top 网站/视频、AI 总结卡；另有设置视图。
- 通过只读连接查询聚合数据填充；实时"当前活动"由采集线程经 `invoke_from_event_loop` 推送。
- 样式与动画见第 10 节。

### 4.11 配置（TOML）与密钥加密（DPAPI）
- 配置文件 `%APPDATA%\ActivityTracker\config.toml`，含：采集兜底轮询间隔、空闲阈值、批量 flush 间隔、排除应用列表、排除域名列表、脱敏规则、AI 开关/粒度、AI base_url/model。
- API Key **不入 TOML**：经 DPAPI 加密后单独存 `%LOCALAPPDATA%\ActivityTracker\key.bin`；运行时解密使用，绝不打印/日志。

## 5. 数据模型

**表 `activity`**（会话粒度）
```
id            INTEGER PK
start_ts      INTEGER  -- Unix 秒
end_ts        INTEGER
duration_sec  INTEGER
app_name      TEXT
process_path  TEXT
window_title  TEXT
edge_url      TEXT     -- 可空
page_title    TEXT     -- 可空
media_title   TEXT     -- 可空
media_player  TEXT     -- 可空
media_status  TEXT     -- playing/paused/null
media_position_sec INTEGER  -- 可空
media_duration_sec INTEGER  -- 可空
is_private    INTEGER  -- 0/1
is_idle       INTEGER  -- 0/1
```
索引：`(start_ts)`、`(app_name, start_ts)`。

**表 `summaries`**：`id, range_start, range_end, granularity(hour/day), text, created_ts, model`。

**表 `meta`**：`key TEXT PK, value TEXT`（含心跳时间戳、schema 版本）。

配置不入库，由 `config.toml` 管理。

## 6. 数据流

1. 用户切换/操作 → 线程 A 收到 WinEvent（或轮询兜底）→ 组 `ActivityEvent`；Edge 时附带 UIA 结果。
2. 播放媒体 → 线程 B 收到 SMTC 事件/采样 → 组 `MediaEvent`。
3. 事件经 mpsc → 线程 C 会话合并 → 内存缓冲。
4. 达到 flush 条件 → 单事务写入 `activity`。
5. UI 显示时 → 只读连接查聚合 → 填充 Bento 卡片。
6. 定时/按需 → AI 线程聚合+脱敏 → 调用接口 → 写 `summaries` → UI 展示。

## 7. 能力边界与降级策略（诚实说明）

| 数据 | 手段 | 可靠度 | 降级 |
|------|------|--------|------|
| 应用/进程/标题/时长 | WinEventHook + 进程查询 | 高 | 轮询兜底 |
| 空闲检测 | GetLastInputInfo | 高 | — |
| Edge URL + 页面标题 | UI Automation | 中（可能显示清理过的 URL/偶发读不到） | 退化为窗口标题 |
| 视频标题+播放状态+进度 | SMTC | 中（取决于站点/播放器是否上报 timeline） | 无进度时记"标题 + 累计时长" |
| InPrivate | 标题/UIA 启发式 | 中（启发式） | 默认不记录 |

**已与用户确认**：视频精确进度只在 SMTC 上报时可得（YouTube/B站/多数播放器均报），否则退化为"看了《标题》共 X 分钟"。此边界符合"不录屏/不 OCR/不抓包"红线。

## 8. 隐私与安全
- 所有原始数据只存本地，永久保存、无清理。
- AI 只收脱敏聚合摘要；支持脱敏域名/标题/关键词、排除应用/域名。
- 托盘一键暂停；隐私模式（InPrivate）默认不记录。
- API Key DPAPI 加密，绑账户，不明文、不打印。
- 不做任何将代码/密钥/原始数据外传的行为（AI 调用除外，且仅脱敏摘要）。

## 9. 轻量化策略与硬指标
- 事件驱动为主，低频轮询兜底（间隔可配）；空闲即停采集。
- 批量合并写入，避免每秒写盘。
- 单进程；Slint 窗口懒创建、隐藏零成本；UIA 仅 Edge 前台时触发。
- 发布期 `opt-level="z"+lto+strip+panic=abort` 压体积。
- **硬指标**：空闲 CPU < 1%、空闲内存 < 50MB、安装包 < 30MB。实现后以实测数据验证（内存/CPU 采样、体积）。

## 10. UI 设计令牌（Bento Grids + 克制动画）

> 来源：用户指定 vibeui.top 的 Bento Grids（风格）与 Motion-Driven（动画）。二者原为 Web（CSS Grid/Tailwind/GSAP/Framer）参考，此处**翻译为 Slint 原生可表达的令牌**；最终配色/细节由用户后续定，本节为基线。

**配色**
- 背景 `#F5F5F7`、卡片 `#FFFFFF`、主文本 `#1D1D1F`、次文本 `#6E6E73`。
- 反馈色：成功 `#22C55E`、错误 `#EF4444`。
- 暗色模式完整支持（背景 `#1D1D1F`/近黑、卡片 `#1C1C1E`）。

**字体**：Inter（无衬线）；中文回退系统 UI 字体；时长/数字用等宽对齐（tabular figures）。

**形态**：大圆角卡片（16–20px）+ 柔和多层阴影（Slint `drop-shadow-blur`/`offset`/低透明度黑）+ 慷慨留白（卡片 gap 12–16px、内边距 16–24px）。Bento 网格 = 大小不一的模块卡。

**动画（取 Motion-Driven 微交互精神，舍重动画）**
- 采纳：hover `scale(1.02)` + 阴影扩展（`animate` 300–400ms ease-out）；开窗卡片淡入 + 上移 8px（轻微错峰）；数值/视图切换平滑过渡；状态色反馈。
- **不做**：视差 3–5 层、滚动 Intersection Observer 动画、重页面转场（与轻量化冲突，且非 Slint 舒适区；Motion-Driven 自身亦标注"避免用于数据仪表板/低功耗设备"）。
- 提供"精简动画"开关（尊重 reduced-motion，低配可关）。

## 11. 错误处理与崩溃恢复
- 采集/媒体/AI 各线程独立，单线程异常不拖垮进程；捕获并记录本地日志（轮转、体积上限）。
- WinRT/UIA 调用失败 → 降级（见第 7 节），不抛给用户。
- 崩溃恢复：启动时用心跳封口遗留会话，清理幻影记录（第 4.6 节）。
- 数据库打开失败/损坏 → 备份原库并新建，提示用户，不静默丢数据。
- AI 调用失败 → 有限重试后标记该时段"未生成"，可手动重试；不影响记录。

## 12. 测试策略
- **纯函数单测**：会话合并、聚合、脱敏/排除、URL/标题解析、时间线生成、心跳封口。
- **存储集成测试**：临时库验证批量写入、WAL 并发读、崩溃恢复清理。
- **AI 载荷测试**：脱敏后 JSON 结构、数值一致性；用 mock/本地桩验证提示词与解析（不实际外发）。
- **系统采集器**：Win32/WinRT/UIA 封薄接口 + 对数据结构 mock 单测；真实抓取靠手动验证脚本（附验证步骤）。
- **轻量化验证**：实测空闲 CPU/内存采样 + 安装包体积，对照第 9 节硬指标。

## 13. 依赖清单（候选 crates）
- `windows`（features：Win32_UI_WindowsAndMessaging、Win32_UI_Accessibility(UIA)、Win32_System_Threading、Win32_Security_Cryptography(DPAPI)、Media_Control(SMTC) 等）
- `slint`、`tray-icon`
- `rusqlite`（`bundled`）
- `serde`、`serde_json`、`toml`
- `ureq`（`rustls` 特性）
- `time`
- `crossbeam-channel`（或 std mpsc）
- 日志：`log` + 轻量后端（如 `simplelog`，带轮转/上限）

> 依赖尽量精简；每引入一个都权衡对体积/内存的影响。

## 14. 默认配置值（已与用户确认，均可在 TOML 改）
- 兜底轮询间隔：5s
- 空闲阈值：60s
- 批量 flush：15s 或 200 条（先到者）
- AI 总结粒度：按天（可选按小时）+ 手动"立即生成"
- 隐私模式（InPrivate）：默认不记录
- 排除应用/域名：默认空

## 15. 未来可选项（v1 不做）
- 本地 Ollama（走同一 OpenAI 兼容接口即可支持，无需改代码）。
- 更丰富的图表/周报/月报。
- 数据导出（Markdown/CSV）。
- MCP Server 暴露给 AI 工具链（Yolo 式）。

---

## 附：待实现计划阶段细化的开放点
- tray-icon 与 Slint winit 后端的事件循环桥接方式。
- SMTC 会话与前台窗口的关联规则（多会话时选哪个）。
- UIA 地址栏定位的具体 AutomationId/策略（随 Edge 版本可能变化，需实测）。
- 脱敏规则的具体表达式/配置格式。







