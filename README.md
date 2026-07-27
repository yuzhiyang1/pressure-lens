# Pressure Lens

[中文](./README.md) | [English](./README_EN.md)

Pressure Lens 是一个面向 Windows 的本地优先认知负荷可视化应用。它聚合键盘输入
强度、修改键比例、应用切换、连续活跃时间和 Agent 上下文占用，生成可解释的当天
工作负荷趋势。

> 当前状态：实验性 Windows MVP。压力分数反映的是工作节奏，不是医学或心理健康诊断。

## 它能做什么

- 在仪表盘中展示当前压力、影响因素、Agent 上下文占用和当天趋势。
- 自动采集 Windows 全局键盘活动、前台应用切换和连续活跃时间，但不记录字符内容。
- 自动读取本机 Codex 会话的结构化 token 指标，不读取会话正文。
- 在桌面上显示透明、始终置顶、默认鼠标穿透的 WebGL 黑洞。
- 用实时桌面采样产生局部引力透镜效果，并在拖动时隔离旧帧，避免残影。
- 通过托盘菜单、快捷键或悬停交互控制黑洞的显示和位置。

## 黑洞视觉

桌面黑洞使用透明 WebGL2 Shader。Schwarzschild 光线轨迹、薄吸积盘交叉、
温度梯度和相对论增亮模型参考并适配自 MIT 许可的
[`s0xDk/ghostty-blackhole`](https://github.com/s0xDk/ghostty-blackhole)。
完整归属信息见 [THIRD_PARTY_NOTICES.md](./THIRD_PARTY_NOTICES.md)。

黑洞会持续漂移和旋转，并在 Inferno、Gargantua、M87* donut、Face-on ember、
Quasar、Blazar 和 Pure lens 七种形态之间平滑变换。形态约每 7～12 秒进入下一种，
完整巡游约 48～82 秒。压力越高，移动范围、旋转速度、形态切换速度和事件视界尺寸
越大；连续累计相位可以避免压力变化时发生瞬移。

## 自动采集与压力模型

应用启动后会监听 `%USERPROFILE%\.codex\sessions`：

- 每 2 秒检查最近活跃的 Codex 会话。
- 使用 `last_token_usage.total_tokens / model_context_window` 计算上下文占用。
- 多个会话同时活跃时，由上下文占用最高的会话驱动 Agent 压力，并展示活跃会话数。
- 遇到正在写入的不完整 JSON 行时，继续使用上一条完整指标。
- 文件未变化时复用内存缓存，避免反复扫描会话正文。

当前压力分数同时考虑输入强度、修改键比例、应用切换、连续工作时长、Agent 上下文
占用、活跃 Agent 数和近期失败次数。其他 Agent 可以通过相同的本地适配器接口接入；
无法提供结构化 token 指标时，只能可靠判断进程是否运行。

## 隐私边界

- 不保存实际输入字符、窗口标题或剪贴板内容。
- 键盘虚拟键值只在内存中计数，写入 SQLite 前只保留聚合数字。
- Codex 采集器只解析 `token_count` 等结构化事件，不保存、展示或分析会话正文。
- 桌面折射只在内存中捕获悬浮窗下方的同尺寸区域，约每秒 12 帧；捕获帧不会落盘、
  不会上传，也不会进入压力评分。
- 拖动期间会暂停并清空桌面折射；松手后只采集新位置，并用短暂淡入消除旧帧残影。
- 黑洞覆盖层已从 Windows 屏幕捕获中排除，避免递归镜像。
- 折射纹理会在四边羽化，不使用覆盖整个桌面的蒙层。
- 数据默认只写入本机 SQLite。
- 黑洞锁定时不会拦截鼠标，只有进入移动模式后才会暂时接收鼠标。

## 交互

- 关闭仪表盘只会将应用隐藏到系统托盘，不会停止采集。
- 托盘菜单可以打开仪表盘、显示或隐藏黑洞、进入移动模式以及退出应用。
- 按 `Ctrl + Alt + M` 可解锁移动模式；拖动并松手后会自动锁定和记住位置。
- 也可以把鼠标停在黑洞中心 2 秒。引力蓄力环完成后，按住黑洞即可拖动；计时期间
  仍保持鼠标穿透。

## 技术结构

| 层 | 技术 | 职责 |
| --- | --- | --- |
| 桌面外壳 | Rust + Tauri 2 | 窗口、托盘、快捷键、本地存储和系统采集 |
| 仪表盘 | HTML + CSS + JavaScript | 压力解释、趋势和可见性控制 |
| 桌面黑洞 | WebGL2 + GLSL | 黑洞渲染、形态变换和桌面折射 |
| 数据 | SQLite | 只保存聚合后的本地指标 |

## 环境要求

- Windows 10 或 Windows 11
- Rust stable（MSVC 工具链）
- Microsoft Edge WebView2 Runtime

本项目的前端不需要 Node.js 构建步骤。

## 本地运行

```powershell
git clone git@github.com:yuzhiyang1/pressure-lens.git
cd pressure-lens\src-tauri
cargo run
```

构建 Release 版本：

```powershell
cd src-tauri
cargo build --release
```

可执行文件会生成到 `src-tauri/target/release/pressure-lens.exe`。

## 验证

```powershell
cd src-tauri
cargo test

cd ..
node --test tests/backdrop-capture-gate.test.cjs
```

第二组测试需要本机安装 Node.js，仅用于运行前端回归测试，不是构建应用的必要条件。

## 许可

仓库当前没有声明项目级开源许可证。第三方代码继续遵循各自许可证，详见
[THIRD_PARTY_NOTICES.md](./THIRD_PARTY_NOTICES.md)。
