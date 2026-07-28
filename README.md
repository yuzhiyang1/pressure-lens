# Pressure Lens

[中文](./README.md) | [English](./README_EN.md)

Pressure Lens 是一个 Windows 本地优先的认知负荷可视化应用。它把键盘节奏、应用
切换、连续活跃和 Agent 上下文等聚合信号，转成可解释、可校准的实时压力与今日历史。

> 压力分数描述工作节奏，不是医学或心理健康诊断。应用不记录输入字符、窗口标题或会话正文。

## 现在已经做到

- 真正的滚动 60 秒窗口，不会在整分钟边界归零跳变。
- 每两秒后台评估、每分钟独立写入 SQLite Journal；打开仪表盘不是保存历史的前提。
- 真实今日曲线、平均值、峰值、高压分钟与自评日总结。
- 自评参与个人校准：第一次最多调整 3 分，重复反馈逐步形成上限 15 分的个人偏移。
- 评分置信度、滚动窗口覆盖度和每个数据源的健康状态。
- 高压时给出一个具体恢复动作，而不是只显示红色警告。
- 设置页支持性能档位、动画/折射强度、安静时段、暂停采集、数据源隐私开关、
  保留天数、开机启动和本地历史清理。
- 单实例、托盘常驻、文件日志、异常退出检测、SQLite WAL 恢复和屏幕外位置保护。

## Agent Provider

所有 Provider 只解析结构化元数据，不把会话内容写入 Pressure Lens：

| Provider | 质量 | 采集方式 |
| --- | --- | --- |
| Codex | 精确 | 读取 `.codex/sessions` 最新 `token_count` 与上下文窗口 |
| Claude Code | 估算 | 读取 `.claude/projects` 最新 assistant usage，以 200k 窗口保守估算 |
| Cursor | 仅活跃状态 | 只检查 `workspaceStorage/state.vscdb` 修改时间，不打开聊天消息库 |

Provider 每 5 秒轮询一次，目录清单最多每 30 秒刷新；日志只读取尾部 512 KB。多个会话
同时活跃时，压力由上下文占用最高的会话驱动，并明确标注指标质量。

## 黑洞视觉语义

桌面黑洞使用透明 WebGL2 Shader。Schwarzschild 光子路径、薄吸积盘、温度梯度和
相对论增亮模型参考并适配自 MIT 许可的
[`s0xDk/ghostty-blackhole`](https://github.com/s0xDk/ghostty-blackhole)，归属见
[THIRD_PARTY_NOTICES.md](./THIRD_PARTY_NOTICES.md)。

黑洞持续流动、漂移和旋转，但形态不再随机乱跳：

- `calm`：第一版 Inferno 倾斜盘；
- `focused`：收紧的 Gargantua 侧视盘；
- `overloaded`：更热、更宽的 Quasar / Blazar；
- `uncertain`：低置信度或暂停时回到稳定的 Inferno 锚点。

状态之间平滑过渡。默认关闭“演示六种形态”；主动开启后会轮换六种造型，
此时形态不再只表达当前压力区间。

## 隐私边界

- 键盘 Hook 只增加计数，虚拟键值在回调结束前丢弃。
- 不读取窗口标题、剪贴板或输入字符。
- SQLite 只保存分钟级聚合数字、自评、设置和运行状态。
- 桌面折射只捕获悬浮窗下方 420×420 区域，在内存中压缩后传给 WebView；不落盘、
  不上传、不参与评分。
- 拖动时暂停并清空折射，旧位置帧即使晚到也会被拒绝。
- 覆盖层会正常出现在系统截图中，便于记录和反馈；折射只在四边羽化的局部窗口内完成，
  不使用全桌面蒙层。
- 暂停采集会冻结最后判断并降低置信度，不会伪装成“压力为零”。

## 性能

性能是发布门禁，而不是可选优化。平衡档限制为 15 FPS、1 FPS 折射采样、最高 2 DPR
和 52 步光线积分；隐藏覆盖层或进入安静时段后会停止渲染和桌面捕获。视觉优先档才会
提高到 30 FPS、最高 2.5 DPR 和 56 步积分。

本机完整进程树（Rust + 两个 WebView + GPU）验收结果：

| 指标 | v0.2.0 实测 | CI 门禁 |
| --- | ---: | ---: |
| 归一化 CPU | 1.11%～2.64% | ≤ 3% |
| GPU（默认压力驱动） | 平均 9.98%，峰值 12.97% | 代表机手工 ≤ 20% |
| GPU（六形态巡游） | 平均 11.71%，峰值 15.20% | 代表机手工 ≤ 20% |
| 私有内存峰值 | 340.38～351.06 MB | ≤ 450 MB |
| 工作集峰值 | 585.92～602.24 MB | ≤ 700 MB |
| 30 秒私有内存变化 | -4.05～1.13 MB | 增长 ≤ 30 MB |

当前视觉策略在 2026-07-28 的一次额外 Release 实测为：CPU 1.04%、私有内存峰值
384.91 MB、工作集峰值 625.59 MB、30 秒私有内存变化 -6.12 MB，仍通过全部门禁。

旧的原始帧实现曾达到约 3.33 GB 私有内存。现在使用压缩帧 IPC、固定缓冲、
`ImageBitmap.close()`、纹理原位更新和消费背压。完整方法见
[docs/performance-budget.md](./docs/performance-budget.md)。

## 交互

- 关闭仪表盘只隐藏到托盘，不停止采集。
- 托盘可打开仪表盘、显示/隐藏黑洞、进入移动模式或退出。
- `Ctrl + Alt + M` 可解锁拖动；也可以在黑洞中心悬停 2 秒后按住拖动。
- 松手后恢复鼠标穿透并事务化保存位置。

## 安装与运行

要求 Windows 10/11 与 Microsoft Edge WebView2 Runtime。

本地运行：

```powershell
git clone git@github.com:yuzhiyang1/pressure-lens.git
cd pressure-lens\src-tauri
cargo run
```

生成 Windows NSIS 安装包：

```powershell
cd pressure-lens
npx --yes @tauri-apps/cli@latest build --bundles nsis
```

产物位于 `src-tauri/target/release/bundle/nsis/`。

## 验证

```powershell
npm ci
npm test

cd src-tauri
cargo fmt -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release

cd ..
.\scripts\windows-native-smoke.ps1 `
  -Executable .\src-tauri\target\release\pressure-lens.exe
.\scripts\measure-performance.ps1 `
  -Executable .\src-tauri\target\release\pressure-lens.exe
```

CI 会执行 Rust、前端单元测试、真实 Chrome E2E、原生单实例/日志冒烟与完整进程树
性能门禁。

## 签名、更新与发布

发布工作流只接受双重签名：

1. Tauri Updater 私钥签名更新包，并在应用中嵌入公开公钥；
2. 受信任的 Windows 代码签名证书为 EXE 和 NSIS 安装包签名并加时间戳。

仓库发布前需要设置：

- Repository variable：`PRESSURE_LENS_UPDATER_PUBKEY`
- Actions secrets：`TAURI_SIGNING_PRIVATE_KEY`、`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`
- Actions secrets：`WINDOWS_CERTIFICATE`（PFX 的 Base64）、`WINDOWS_CERTIFICATE_PASSWORD`

缺少任一项，Release 工作流会失败并拒绝发布未签名安装包。本地验收安装包不会冒充
受信任发行版；正式发布需由证书持有人配置上述凭据。

## 许可

仓库尚未声明项目级开源许可证。第三方代码继续遵循各自许可证，见
[THIRD_PARTY_NOTICES.md](./THIRD_PARTY_NOTICES.md)。
