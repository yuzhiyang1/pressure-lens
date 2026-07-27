const invoke = window.__TAURI__?.core?.invoke;
const orbit = document.querySelector("#orbit");
const visualStatus = document.querySelector("#visual-status");
const shapeParameter = new URLSearchParams(location.search).get("shape");
const shapeOverride = shapeParameter == null
  ? null
  : Math.max(0, Math.min(6.999, Number(shapeParameter) || 0));
let dashboardPressure = 0;

function formatTokens(value) {
  if (value == null) {
    return "等待 token_count";
  }
  if (value >= 1_000_000) {
    return `${(value / 1_000_000).toFixed(1)}m`;
  }
  if (value >= 1_000) {
    return `${(value / 1_000).toFixed(1)}k`;
  }
  return String(value);
}

function levelLabel(level) {
  return {
    calm: "负荷平稳",
    elevated: "负荷正在升高",
    high: "建议主动降载或休息",
  }[level] ?? "正在建立今天的基线";
}

function renderReasons(reasons) {
  const list = document.querySelector("#reasons");
  const fragments = reasons.map((reason) => {
    const item = document.createElement("li");
    item.textContent = reason;
    return item;
  });
  list.replaceChildren(...fragments);
}

function render(snapshot) {
  const score = Math.round(snapshot.pressure.score);
  dashboardPressure = score / 100;
  document.documentElement.style.setProperty("--pressure", `${score}%`);
  document.querySelector("#score").textContent = score;
  document.querySelector("#level").textContent = levelLabel(snapshot.pressure.level);
  orbit.dataset.level = snapshot.pressure.level;
  document.querySelector("#keys").textContent = snapshot.sample.keys_per_minute;
  document.querySelector("#keyboard-status").textContent = snapshot.sample.keyboard_hook_ready
    ? "次 / 分钟 · 采集已就绪"
    : `采集未就绪${snapshot.sample.keyboard_hook_error ? ` · 错误 ${snapshot.sample.keyboard_hook_error}` : ""}`;
  document.querySelector("#backspace").textContent =
    `${Math.round(snapshot.sample.backspace_ratio * 100)}%`;
  document.querySelector("#context").textContent =
    snapshot.sample.agent_context_percent == null
      ? "待机"
      : `${Math.round(snapshot.sample.agent_context_percent)}%`;
  const agentSource = snapshot.sample.agent_source ?? "Agent";
  document.querySelector("#agents").textContent =
    `${agentSource} 自动采集 · ${snapshot.sample.active_agents} 个活跃会话`;
  document.querySelector("#agent-source-status").textContent =
    snapshot.sample.agent_automatic
      ? `${agentSource} 监听中`
      : "等待自动采集器";
  document.querySelector("#agent-token-detail").textContent =
    snapshot.sample.agent_context_tokens == null
      ? "当前没有活跃会话"
      : `${formatTokens(snapshot.sample.agent_context_tokens)} / ${formatTokens(snapshot.sample.agent_context_window)} tokens`;
  document.querySelector("#active").textContent =
    `${Math.round(snapshot.sample.continuous_active_seconds / 60)} 分钟`;
  document.querySelector("#switches").textContent =
    `${snapshot.sample.app_switches} 次应用切换`;
  visualStatus.textContent = `压力映射 ${score} / 100`;

  const reasons = snapshot.pressure.reasons.length
    ? snapshot.pressure.reasons
    : ["当前没有明显的高负荷信号"];
  renderReasons(reasons);
}

function browserPreviewSnapshot() {
  // 仅用于浏览器视觉验收；Tauri 中始终读取本机真实聚合数据。
  const previewScore = Math.max(
    0,
    Math.min(100, Number(new URLSearchParams(location.search).get("preview")) || 64),
  );
  return {
    pressure: {
      score: previewScore,
      level: previewScore >= 70 ? "high" : previewScore >= 40 ? "elevated" : "calm",
      reasons: [
        "连续活跃 104 分钟，恢复窗口正在变窄",
        "Agent 上下文达到 71%，切换成本开始上升",
        "输入节奏高于今天的个人基线",
      ],
    },
    sample: {
      keys_per_minute: 156,
      keyboard_hook_ready: true,
      keyboard_hook_error: null,
      backspace_ratio: 0.087,
      agent_context_percent: 71,
      agent_context_tokens: 142_000,
      agent_context_window: 200_000,
      agent_source: "Codex",
      agent_automatic: true,
      active_agents: 3,
      continuous_active_seconds: 6240,
      app_switches: 26,
    },
  };
}

async function refresh() {
  if (!invoke) {
    render(browserPreviewSnapshot());
    return;
  }

  try {
    render(await invoke("get_snapshot"));
  } catch (error) {
    document.querySelector("#level").textContent = `读取本地状态失败：${error}`;
  }
}

async function refreshOverlayButton() {
  const button = document.querySelector("#toggle-overlay");
  if (!invoke) {
    button.textContent = "桌面黑洞已显示";
    return;
  }

  try {
    const visible = await invoke("get_overlay_visible");
    button.textContent = visible ? "隐藏桌面黑洞" : "显示桌面黑洞";
  } catch {
    button.textContent = "覆盖层状态不可用";
  }
}

document.querySelector("#toggle-overlay").addEventListener("click", async () => {
  if (!invoke) {
    return;
  }
  const visible = await invoke("get_overlay_visible");
  await invoke("set_overlay_visible", { visible: !visible });
  await refreshOverlayButton();
});

document.querySelector("#self-report").addEventListener("click", async (event) => {
  const button = event.target.closest("button[data-value]");
  const value = Number(button?.dataset.value);
  if (!button || !value) {
    return;
  }

  if (invoke) {
    await invoke("record_self_report", { value });
  }

  // 明确反馈当前选择，同时只在 Tauri 中执行真实落盘。
  document
    .querySelectorAll("#self-report button")
    .forEach((item) => item.setAttribute("aria-pressed", String(item === button)));
  document.querySelector("#report-status").textContent = invoke
    ? "已保存到本机，后续评分会参考这次校准。"
    : "预览选择已更新；Tauri 运行时会保存到本机。";
});

refresh();
refreshOverlayButton();
setInterval(refresh, 2000);

window.PressureBlackHole
  .start(
    document.querySelector("#dashboard-blackhole"),
    () => dashboardPressure,
    { maximumDpr: 1.25, framesPerSecond: 30, shapeOverride },
  )
  .then(() => orbit.classList.add("is-ready"))
  .catch((error) => {
    visualStatus.textContent = `引力模型不可用：${error.message}`;
  });
