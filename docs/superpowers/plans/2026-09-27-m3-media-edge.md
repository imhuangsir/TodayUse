# M3 媒体(SMTC) + Edge(UIA) 采集 实现计划

> inline 前台执行。构建前缀 `source /d/rust/env.sh &&`。

**目标：** 采集正在播放的媒体（标题/状态/进度，SMTC）与 Edge 当前标签 URL/页面标题（UI Automation），接入 M2 采集循环富化会话。

**技术栈新增：** windows crate 加 `Media_Control`、`Foundation`（SMTC）、`UI_Accessibility`/`System_Com`（UIA）features。

**规格：** 第 4.3（Edge UIA）/4.4（SMTC）节。

## 全局约束
- 所有 Win32/WinRT 调用失败一律降级（返回 None），**绝不 panic、绝不打断采集循环**（规格第 11 节）。
- 沿用 M1/M2 约束。

## 诚实的验证边界（重要）
- **SMTC**：可 runtime 冒烟验证"无媒体时返回 None、不崩溃"；有媒体时的字段正确性需用户放视频/音乐后交互验证。
- **Edge UIA**：地址栏元素随 Edge 版本变化，需用户开着 Edge 交互验证。M3 用"找 value 形似 URL 的 Edit 控件"启发式（比猜 AutomationId 稳），但**能否真读到取决于本机 Edge**，标为需用户验证。

## 文件结构
- `Cargo.toml` — windows features 增补。
- `src/media.rs`（cfg windows）— `MediaSnapshot` + `media_snapshot()->Option<MediaSnapshot>`（SMTC）。
- `src/edge.rs`（cfg windows）— `edge_url_title(pid)->Option<(String,Option<String>)>`（UIA 启发式）。
- `src/collector.rs` — run_for 循环里：每轮 media_snapshot→on_media；前台是 msedge 时 edge_url_title→并入事件。
- `src/lib.rs` — 声明模块。

## 任务
### T1：SMTC 媒体（media.rs）+ 接入 + 冒烟
- `media.rs`：RequestAsync().get()→GetCurrentSession→标题/状态/timeline(position/end 秒)；失败全 None。
- collector 每轮调用，Some 则 `sb.on_media`。
- 验证：cargo build/clippy 干净；`collect` 冒烟不崩（无媒体返回 None）。
- Commit：`feat(media): SMTC 媒体标题/状态/进度采集 + 接入采集循环`

### T2：Edge UIA URL/标题（edge.rs）+ 接入
- `edge.rs`：CoInitializeEx→CUIAutomation→ElementFromHandle(前台 hwnd)→按 ControlType=Edit 且 value 含"://"或形似域名 的启发式找地址栏→读 ValuePattern。失败全 None。
- collector：前台进程名含 "msedge" 时调用，把 url/page_title 并入 build_event（需扩展 build_event 或直接构造事件）。
- 验证：cargo build/clippy 干净；`collect` 冒烟不崩。真读取正确性标为需用户开 Edge 验证。
- Commit：`feat(edge): UIA 读取 Edge 地址栏 URL/标题(启发式) + 接入`
