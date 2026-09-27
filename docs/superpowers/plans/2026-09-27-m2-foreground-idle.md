# M2 前台采集 + 空闲检测 实现计划

> **面向执行者：** inline 前台执行（子代理不可用）。步骤用复选框。构建命令一律 `source /d/rust/env.sh && cd /d/activity-tracker && cargo ...`。

**目标：** 让程序真正记录本机前台应用/进程/窗口标题/时长，并做系统级空闲检测；接入 M1 的会话合并与存储。

**架构决策（Ruling，偏离规格的事件驱动）：** 规格设计的是 `SetWinEventHook` 事件驱动。M2 改用**低频轮询**（`GetForegroundWindow` + `GetLastInputInfo`，默认 5s）——原因：(1) 轮询无 unsafe 回调/全局状态，可 runtime 冒烟验证；(2) 5s 轮询 CPU 可忽略，满足 <1%；(3) WinEventHook 回调需交互切窗才能验证，当前无法交互确认。事件驱动留作后续优化。若错代价=响应延迟最多一个轮询周期，用户可见易调。

**技术栈新增：** `windows` crate（gnu 目标兼容），选择性 features。

**规格：** `docs/superpowers/specs/2026-09-27-activity-tracker-design.md`（第 4.1/4.2 节）。

## 全局约束
- 沿用 M1 全部约束（UTC 秒、字段名、commit 结尾附 Co-Authored-By）。
- windows crate 用固定小版本；features 按需最小集。
- Win32 薄封装单独成模块，纯逻辑与之分离且可单测。
- 每任务 `cargo test` + `cargo clippy` 干净后再 commit。

## 文件结构
- `Cargo.toml` — 加 `[target.'cfg(windows)'.dependencies] windows`。
- `src/platform.rs` — Win32 薄封装：`foreground_snapshot()`、`last_input_tick()`、`now_tick()`（含 `ForegroundInfo`）。仅 compile 验证。
- `src/collector.rs` — 纯逻辑（`is_idle`/`basename`/`is_excluded`/`build_event`）+ 采集循环 `run_for`。
- `src/main.rs` — 加 `collect` 子命令（跑 N 秒后退出，供冒烟）。
- `src/lib.rs` — 声明新模块。

---

### 任务 1：windows 依赖 + Win32 薄封装（platform.rs）
- 在 Cargo.toml 加 `[target.'cfg(windows)'.dependencies]` 的 `windows`（features：Win32_Foundation、Win32_UI_WindowsAndMessaging、Win32_System_Threading、Win32_UI_Input_KeyboardAndMouse、Win32_System_SystemInformation）。
- `src/platform.rs`：`ForegroundInfo{ title, process_path, pid }`；`foreground_snapshot()->Option<ForegroundInfo>`（GetForegroundWindow→GetWindowTextW→GetWindowThreadProcessId→OpenProcess+QueryFullProcessImageNameW，句柄用完 CloseHandle）；`last_input_tick()->u32`（GetLastInputInfo）；`now_tick()->u32`（GetTickCount）。全部 unsafe 封装内消化，对外安全接口。
- 验证：`cargo build` 通过（windows crate 在 gnu 编过）。不单测。
- Commit：`feat(platform): Win32 前台窗口/进程/空闲tick 薄封装`

### 任务 2：采集纯逻辑 + 单测（collector.rs 上半）
- `is_idle(last_input_tick:u32, now_tick:u32, threshold_ms:u32)->bool`（用 `now.wrapping_sub(last) >= threshold_ms`，处理 GetTickCount 回绕）。
- `basename(path:&str)->String`（取文件名做 app_name，去 .exe 可留，统一用带扩展名的文件名）。
- `is_excluded(app_name:&str, process_path:&str, excluded:&[String])->bool`（大小写不敏感匹配 app_name 或路径含关键字）。
- `build_event(ts, info:&ForegroundInfo, is_idle)->ActivityEvent`（app_name=basename(path) 回退窗口所属；edge_url/page_title 置 None 留 M3）。
- 单测覆盖：idle 正常/回绕、basename、is_excluded 命中/不命中。
- Commit：`feat(collector): 空闲判定/进程名/排除 纯逻辑 + 单测`

### 任务 3：采集循环 + collect 子命令 + 冒烟（collector.rs 下半 + main.rs）
- `run_for(cfg:&Config, db_path:&str, seconds:u64)`：打开 Storage；循环每 `poll_interval_sec`：算 idle→若排除则跳过→`build_event`→`SessionBuilder.on_activity`→关闭的会话入缓冲；按 `flush_*` 批量 `insert_sessions`；周期 `set_heartbeat`；到时长后 `finish` 收尾 flush。启动先 `recover_phantom`。
- `main.rs` 加 `collect <db> <seconds>` 子命令调用 `run_for`。
- 验证：`cargo test`（纯逻辑）+ `cargo clippy` 干净；**runtime 冒烟**：`collect <db> 6` 跑 6 秒后 `report <db> <今日>`，确认捕获到当前前台窗口（本会话的终端/编辑器）有非零记录。
- Commit：`feat(collector): 轮询采集循环 + collect 子命令`

## 自检
- 纯逻辑与 Win32 分离，纯逻辑可测；采集循环靠 runtime 冒烟。
- 诚实边界：本里程碑只在本机 runtime 冒烟验证"能记录当前前台窗口"；不同应用/多显示器等场景未穷尽。

