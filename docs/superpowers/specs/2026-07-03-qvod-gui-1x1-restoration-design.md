# QVOD GUI 1:1 还原设计文档

## 概述

将现有 egui 播放器 GUI 从功能原型还原为快播 QvodPlayer 6.x 经典外观。
左侧视频播放区 + 右侧网络任务列表的双栏布局，深蓝灰色调，全部控件自定义绘制。

## 模块架构

### 新增 `skin/` 模块

```
qvs-gui/src/
├── skin/
│   ├── mod.rs          — SkinEngine trait + re-exports
│   ├── qvod6.rs        — Qvod 6.x 皮肤实现 (硬编码)
│   └── palette.rs      — 色板常量
├── theme.rs            → 瘦身为 SkinEngine 代理 (当前 Qvod6Skin)
├── app.rs              — 布局改为左视频右列表
├── player.rs           — 视频区重写 (Qvod 6x 风格叠加层)
├── controls.rs         — 控制栏完全自定义绘制
├── playlist.rs         — 右侧列表重写 (双标签页: 正在播放/网络任务)
├── overlay.rs          — 覆盖层调用 skin 绘制
└── ...
```

### SkinEngine Trait

```rust
pub trait SkinEngine: Send + Sync {
    fn name(&self) -> &str;
    fn apply_style(&self, ctx: &egui::Context);

    // 窗口
    fn draw_title_bar(&self, ui: &mut egui::Ui, title: &str) -> TitleBarAction;

    // 控制栏
    fn draw_play_button(&self, ui: &mut egui::Ui, playing: bool) -> bool;
    fn draw_stop_button(&self, ui: &mut egui::Ui) -> bool;
    fn draw_time_display(&self, ui: &mut egui::Ui, position_ms: u64, duration_ms: u64);
    fn draw_progress_bar(&self, ui: &mut egui::Ui, progress: f32, buffered: f32) -> Option<f32>;
    fn draw_volume_control(&self, ui: &mut egui::Ui, volume: &mut f32, muted: &mut bool);
    fn draw_fullscreen_button(&self, ui: &mut egui::Ui) -> bool;

    // 右侧列表
    fn draw_tab_bar(&self, ui: &mut egui::Ui, tabs: &[&str], active: &mut usize);
    fn draw_task_entry(&self, ui: &mut egui::Ui, entry: &TaskEntry, index: usize) -> TaskAction;

    // 叠加层
    fn draw_buffering_overlay(&self, ui: &mut egui::Ui, area: egui::Rect);
    fn draw_error_overlay(&self, ui: &mut egui::Ui, area: egui::Rect, msg: &str);
    fn draw_info_overlay(&self, ui: &mut egui::Ui, area: egui::Rect, info: &str);

    // 右键菜单
    fn draw_context_menu(&self, ui: &mut egui::Ui) -> Vec<ContextMenuAction>;

    // 未来扩展
    // fn load_qvs_package(&mut self, path: &Path) -> Result<(), QvodError>;
}

#[derive(Debug)]
pub enum TitleBarAction {
    None,
    Minimize,
    Maximize,
    Close,
    Drag,
}

#[derive(Debug)]
pub enum TaskAction {
    None,
    Select(usize),
    Play(usize),
    Remove(usize),
    ContextMenu(usize, egui::Rect),
}

#[derive(Debug, Clone)]
pub struct TaskEntry {
    pub title: String,
    pub uri: String,
    pub status: TaskStatus,
    pub progress: f64,
    pub downloaded: u64,
    pub total: u64,
    pub speed_down: f64,
    pub speed_up: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TaskStatus {
    Downloading,
    Paused,
    Completed,
    Error(String),
}

#[derive(Debug)]
pub enum ContextMenuAction {
    Play,
    Pause,
    Stop,
    Restart,
    Remove,
    Properties,
    PriorityHigh,
    PriorityNormal,
    PriorityLow,
    SpeedLimit(u32),
}
```

## 色板 (palette.rs)

```rust
// 主色
pub const BG_GRADIENT_TOP: egui::Color32 = egui::Color32::from_rgb(0x1A, 0x1A, 0x2E);
pub const BG_GRADIENT_BOTTOM: egui::Color32 = egui::Color32::from_rgb(0x16, 0x21, 0x3E);
pub const VIDEO_BG: egui::Color32 = egui::Color32::from_rgb(0x00, 0x00, 0x00);
pub const CONTROL_BAR_BG: egui::Color32 = egui::Color32::from_rgb(0x0F, 0x0F, 0x23);
pub const SIDEBAR_BG: egui::Color32 = egui::Color32::from_rgb(0x1E, 0x1E, 0x30);
pub const TITLE_BAR_BG: egui::Color32 = egui::Color32::from_rgb(0x12, 0x12, 0x2A);

// 控件色
pub const BTN_DEFAULT: egui::Color32 = egui::Color32::from_rgb(0xE8, 0xE8, 0xE8);
pub const BTN_HOVER: egui::Color32 = egui::Color32::from_rgb(0x4F, 0xC3, 0xF7);
pub const BTN_ACTIVE: egui::Color32 = egui::Color32::from_rgb(0x02, 0x88, 0xD1);

// 进度/音量条
pub const PROGRESS_BG: egui::Color32 = egui::Color32::from_rgb(0x4A, 0x4A, 0x6A);
pub const PROGRESS_FILL: egui::Color32 = egui::Color32::from_rgb(0x00, 0xBC, 0xD4);
pub const PROGRESS_BUFFERED: egui::Color32 = egui::Color32::from_rgb(0x3A, 0x3A, 0x5A);

// 文字
pub const TEXT_PRIMARY: egui::Color32 = egui::Color32::from_rgb(0xE0, 0xE0, 0xE0);
pub const TEXT_HIGHLIGHT: egui::Color32 = egui::Color32::from_rgb(0xFF, 0xFF, 0xFF);
pub const TEXT_SECONDARY: egui::Color32 = egui::Color32::from_rgb(0x90, 0x90, 0x90);

// 语义色
pub const ERROR: egui::Color32 = egui::Color32::from_rgb(0xFF, 0x52, 0x52);
pub const SUCCESS: egui::Color32 = egui::Color32::from_rgb(0x69, 0xF0, 0xAE);
pub const WARNING: egui::Color32 = egui::Color32::from_rgb(0xFF, 0xC1, 0x07);
```

## 窗口布局

### 整体

- `eframe::NativeOptions.viewport.decorations = false`，全自定义窗口绘制
- 窗口最小尺寸: 960x600
- 默认尺寸: 1200x800

### 标题栏 (TitleBar)

```
┌─────────────────────────────────────────────────────────────┬────┬────┬────┐
│  [Q icon] QVOD Player                         (标题/状态)  │ ── │ □  │ ✕  │
└─────────────────────────────────────────────────────────────┴────┴────┴────┘
```

- 高度: 32px
- 左侧显示应用图标 + 名称
- 拖拽区域 (整个标题栏)
- 右侧三个系统按钮，hover 变色

### 菜单栏

```
┌──────┬──────┬──────┬──────┬──────┐
│ 文件  │ 播放 │ 控制 │ 设置  │ 帮助 │
└──────┴──────┴──────┴──────┴──────┘
```

egui `MenuBar`，Qvod 6.x 文字菜单，点击弹出下拉。

### 主区域

```
┌──────────────────────────────────────┬─────────────────────────────┐
│                                      │  [正在播放] [网络任务]     │
│                                      │                             │
│                                      │  ┌───────────────────────┐ │
│           视频区                      │  │ ▶ movie1.rmvb        │ │
│           (70% 宽度)                  │  │ [████░░░░░░] 45%     │ │
│                                       │  │ 12.3/18.5MB ↓235KB/s │ │
│          黑色背景                     │  ├───────────────────────┤ │
│          缓冲/信息叠加层              │  │ ▶ movie2.mp4         │ │
│                                       │  │ [██████████] 100%    │ │
│                                       │  │ 已完成 ✓             │ │
│                                       │  └───────────────────────┘ │
│                                       │                             │
└──────────────────────────────────────┴─────────────────────────────┘
```

- 视频区: 左侧 70%，`set_width_ratio(0.7)`
- 列表区: 右侧 30%，用 `egui::SidePanel::right("task_list").resizable(true)`，min 280px, max 400px

### 控制栏

```
┌────────────────────────────────────────────────────────────────────────┐
│ [▶/⏸] [■]  |  00:12:34/01:23:45  |  [══════●═══════════════]  | [🔊][══●══] | [□] │
└────────────────────────────────────────────────────────────────────────┘
```

- 高度: 48px (含 4px 上边距分割线)
- 所有控件自定义绘制

## 右键菜单

| 场景 | 菜单项 |
|------|--------|
| 视频区 | 播放/暂停, 停止, ─, 全屏, ─, 画面比例 (4:3/16:9/原始), ─, 设置, 关于 |
| 列表项 | 播放, 暂停, 停止, ─, 重新开始, 删除, ─, 属性, ─, 优先下载 (高/普通/低), 上传限速 |
| 进度条 | ─ (无右键菜单) |

## 业务规则

1. 播放中点击列表项 → 切换到新资源, 自动开始
2. 拖拽进度条 → 实时显示 tooltip 时间, 松手触发 seek
3. 完成的任务显示绿色勾, 可右键删除
4. 缓冲中显示圆形旋转动画（Painter 每帧旋转弧线，8 段渐变透明度）
5. 错误时显示红色叠加层 + 错误消息
6. 网络任务列表自动按状态排序: 下载中 > 已暂停 > 已完成

## 验收标准

- [ ] cargo build --workspace 通过
- [ ] cargo test --workspace 全部通过
- [ ] cargo clippy --workspace -- -D warnings 无警告
- [ ] cargo fmt --check 通过
- [ ] GUI 窗口无原生装饰, 自定义绘制
- [ ] 色板准确匹配 Qvod 6.x
- [ ] 控制栏全部控件可交互
- [ ] 播放列表双标签切换
- [ ] 右键菜单功能正常
- [ ] SkinEngine trait 设计完整, 预留 .qvs 加载接口注释
