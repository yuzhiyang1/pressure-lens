const invoke = window.__TAURI__?.core?.invoke;
const listen = window.__TAURI__?.event?.listen;
const orbit = document.querySelector("#orbit");
const visualStatus = document.querySelector("#visual-status");
const settingsPreviewStage = document.querySelector("#settings-preview-stage");
const shapeParameter = new URLSearchParams(location.search).get("shape");
const shapeOverride = shapeParameter == null
  ? null
  : Math.max(0, Math.min(6.999, Number(shapeParameter) || 0));

let dashboardPressure = 0;
let dashboardVisualState = "uncertain";
let currentSettings = null;
let settingsDirty = false;
let rendererController = null;
let settingsRendererController = null;
let settingsRendererPromise = null;
let recoveryTimer = null;

const defaultSettings = Object.freeze({
  schema_version: 1,
  collection_paused: false,
  collect_keyboard: true,
  collect_app_switches: true,
  collect_agents: true,
  animation_intensity: 0.65,
  lens_intensity: 0.55,
  performance_mode: "balanced",
  quiet_hours: {
    enabled: false,
    start: "22:00",
    end: "08:00",
  },
  retention_days: 30,
  launch_at_startup: false,
  decorative_shape_tour: false,
});

function formatTokens(value) {
  if (value == null) {
    return "等待 token 指标";
  }
  if (value >= 1_000_000) {
    return `${(value / 1_000_000).toFixed(1)}m`;
  }
  if (value >= 1_000) {
    return `${(value / 1_000).toFixed(1)}k`;
  }
  return String(value);
}

function levelLabel(level, snapshot) {
  if (snapshot.sample.collection_paused) {
    return "采集已暂停 · 保留最后判断";
  }
  if (snapshot.quiet_hours_active) {
    return "安静时段 · 黑洞动画已休眠";
  }
  return {
    calm: "负荷平稳",
    elevated: "负荷正在升高",
    high: "建议主动降载或休息",
  }[level] ?? "正在建立今天的基线";
}

function confidenceLabel(level) {
  return {
    high: "高置信度",
    medium: "中等置信度",
    low: "低置信度",
  }[level] ?? "正在评估";
}

function renderReasons(reasons) {
  const list = document.querySelector("#reasons");
  // 首屏只保留最有决策价值的三个原因，完整趋势仍由下方遥测与历史承接。
  const values = (reasons.length ? reasons : ["当前没有明显的高负荷信号"]).slice(0, 3);
  const fragments = values.map((reason) => {
    const item = document.createElement("li");
    item.textContent = reason;
    return item;
  });
  list.replaceChildren(...fragments);
}

function renderSourceHealth(sources = []) {
  const list = document.querySelector("#source-health");
  const fragments = sources.map((source) => {
    const item = document.createElement("li");
    item.dataset.status = source.status;
    const label = document.createElement("strong");
    label.textContent = source.label;
    const detail = document.createElement("span");
    detail.textContent = source.detail;
    item.append(label, detail);
    return item;
  });
  list.replaceChildren(...fragments);

  const healthy = sources.filter((source) => source.status === "healthy").length;
  const errors = sources.filter((source) => source.status === "error").length;
  document.querySelector("#health-overview").textContent = errors
    ? `${errors} 个异常`
    : `${healthy}/${sources.length} 正常`;
}

function renderAdvice(advice) {
  const panel = document.querySelector("#advice-panel");
  panel.hidden = !advice;
  if (!advice) {
    return;
  }
  document.querySelector("#advice-title").textContent = advice.title;
  document.querySelector("#advice-detail").textContent = advice.detail;
  const action = document.querySelector("#start-recovery");
  action.dataset.minutes = advice.suggested_minutes ?? 2;
  action.textContent = `开始 ${advice.suggested_minutes ?? 2} 分钟恢复`;
}

function render(snapshot) {
  const score = Math.round(snapshot.pressure.score);
  const confidence = Math.round(snapshot.pressure.confidence * 100);
  dashboardPressure = score / 100;
  dashboardVisualState = snapshot.pressure.visual_state ?? "uncertain";
  document.documentElement.style.setProperty("--pressure", `${score}%`);
  document.querySelector("#score").textContent = score;
  document.querySelector("#settings-preview-score").textContent = score;
  document.querySelector("#level").textContent = levelLabel(snapshot.pressure.level, snapshot);
  orbit.dataset.level = snapshot.pressure.level;
  document.querySelector("#keys").textContent = snapshot.sample.keys_per_minute;
  const keyboardHealth = snapshot.source_health.find((source) => source.id === "keyboard");
  document.querySelector("#keyboard-status").textContent =
    keyboardHealth?.status === "paused"
      ? "已按隐私设置暂停"
      : snapshot.sample.collection_paused
    ? "采集已暂停"
    : snapshot.sample.keyboard_hook_ready
      ? `次 / 滚动 60 秒 · 覆盖 ${snapshot.sample.window_coverage_seconds}s`
      : `采集未就绪${snapshot.sample.keyboard_hook_error ? ` · 错误 ${snapshot.sample.keyboard_hook_error}` : ""}`;
  document.querySelector("#backspace").textContent =
    `${Math.round(snapshot.sample.backspace_ratio * 100)}%`;
  document.querySelector("#context").textContent =
    snapshot.sample.agent_context_percent == null
      ? "待机"
      : `${Math.round(snapshot.sample.agent_context_percent)}%`;
  const agentSource = snapshot.sample.agent_source ?? "Agent";
  const metricQuality = {
    exact: "精确指标",
    estimated: "估算指标",
    activity_only: "仅活跃状态",
    unavailable: "暂无指标",
  }[snapshot.sample.agent_metric_quality] ?? "暂无指标";
  document.querySelector("#agents").textContent =
    `${agentSource} · ${metricQuality} · ${snapshot.sample.active_agents} 个会话`;
  document.querySelector("#agent-source-status").textContent =
    snapshot.sample.agent_automatic ? `${agentSource} 自动采集中` : "等待 Provider";
  document.querySelector("#agent-token-detail").textContent =
    snapshot.sample.agent_context_tokens == null
      ? metricQuality
      : `${formatTokens(snapshot.sample.agent_context_tokens)} / ${formatTokens(snapshot.sample.agent_context_window)} tokens`;
  document.querySelector("#active").textContent =
    `${Math.round(snapshot.sample.continuous_active_seconds / 60)} 分钟`;
  document.querySelector("#switches").textContent =
    `${snapshot.sample.app_switches} 次应用切换`;
  visualStatus.textContent =
    `${dashboardVisualState.toUpperCase()} · 压力 ${score} · ${confidenceLabel(snapshot.pressure.confidence_level)}`;

  document.querySelector("#confidence-value").textContent = `${confidence}%`;
  document.querySelector("#confidence-bar").style.width = `${confidence}%`;
  document.querySelector("#confidence-detail").textContent =
    `${confidenceLabel(snapshot.pressure.confidence_level)} · 滚动窗口覆盖 ${snapshot.sample.window_coverage_seconds}/60 秒`;
  const adjustment = snapshot.pressure.calibration_adjustment;
  document.querySelector("#calibration-detail").textContent =
    snapshot.pressure.calibration_reports
      ? `已有 ${snapshot.pressure.calibration_reports} 次自评，个人校准 ${adjustment >= 0 ? "+" : ""}${adjustment.toFixed(1)} 分`
      : "尚无个人校准；第一次自评最多只调整 3 分";

  renderReasons(snapshot.pressure.reasons);
  renderSourceHealth(snapshot.source_health);
  renderAdvice(snapshot.pressure.advice);
  const recoveryBanner = document.querySelector("#recovery-banner");
  recoveryBanner.hidden = !snapshot.recovery_notice;
  recoveryBanner.textContent = snapshot.recovery_notice ?? "";
  rendererController?.setPaused(Boolean(snapshot.quiet_hours_active));
}

function renderHistory(history = [], summary) {
  const line = document.querySelector("#history-line");
  const area = document.querySelector("#history-area");
  const empty = document.querySelector("#history-empty");
  if (!history.length) {
    line.setAttribute("d", "");
    area.setAttribute("d", "");
    empty.hidden = false;
  } else {
    // X 轴按真实本地时间分布，而不是把缺失样本伪装成连续数据。
    const points = history.map((point) => {
      const date = new Date(point.recorded_at);
      const minute = date.getHours() * 60 + date.getMinutes() + date.getSeconds() / 60;
      return {
        x: (minute / 1439) * 1000,
        y: 190 - Math.max(0, Math.min(100, point.score)) * 1.75,
      };
    });
    const path = points
      .map((point, index) => `${index ? "L" : "M"} ${point.x.toFixed(1)} ${point.y.toFixed(1)}`)
      .join(" ");
    line.setAttribute("d", path);
    const first = points[0];
    const last = points.at(-1);
    area.setAttribute("d", `${path} L ${last.x.toFixed(1)} 200 L ${first.x.toFixed(1)} 200 Z`);
    empty.hidden = true;
  }

  const day = summary ?? {
    covered_minutes: 0,
    average_score: 0,
    peak_score: 0,
    high_minutes: 0,
    self_report_count: 0,
    headline: "正在建立今天的总结。",
  };
  document.querySelector("#history-coverage").textContent =
    `${day.covered_minutes} 分钟真实覆盖`;
  document.querySelector("#summary-average").textContent =
    day.sample_count ? Math.round(day.average_score) : "--";
  document.querySelector("#summary-peak").textContent =
    day.sample_count ? Math.round(day.peak_score) : "--";
  document.querySelector("#summary-high").textContent = day.high_minutes;
  document.querySelector("#summary-reports").textContent = day.self_report_count;
  document.querySelector("#summary-headline").textContent = day.headline;
}

function applySettingsToForm(settings, { force = false } = {}) {
  if (settingsDirty && !force) {
    return;
  }
  currentSettings = structuredClone(settings);
  document.querySelector("#performance-mode").value = settings.performance_mode;
  document.querySelector("#animation-intensity").value = Math.round(settings.animation_intensity * 100);
  document.querySelector("#lens-intensity").value = Math.round(settings.lens_intensity * 100);
  document.querySelector("#decorative-shape-tour").checked = settings.decorative_shape_tour;
  document.querySelector("#collection-paused").checked = settings.collection_paused;
  document.querySelector("#collect-keyboard").checked = settings.collect_keyboard;
  document.querySelector("#collect-app-switches").checked = settings.collect_app_switches;
  document.querySelector("#collect-agents").checked = settings.collect_agents;
  document.querySelector("#quiet-enabled").checked = settings.quiet_hours.enabled;
  document.querySelector("#quiet-start").value = settings.quiet_hours.start;
  document.querySelector("#quiet-end").value = settings.quiet_hours.end;
  document.querySelector("#retention-days").value = settings.retention_days;
  document.querySelector("#launch-at-startup").checked = settings.launch_at_startup;
  updateRangeOutputs();
  settingsDirty = false;
  rendererController?.setPolicy(settings.performance_mode, {
    animationIntensity: settings.animation_intensity,
    lensIntensity: 0,
    decorativeShapeTour: settings.decorative_shape_tour,
  });
  settingsRendererController?.setPolicy(settings.performance_mode, {
    animationIntensity: settings.animation_intensity,
    lensIntensity: settings.lens_intensity,
    decorativeShapeTour: settings.decorative_shape_tour,
  });
}

function readSettingsForm() {
  return {
    schema_version: currentSettings?.schema_version ?? 1,
    collection_paused: document.querySelector("#collection-paused").checked,
    collect_keyboard: document.querySelector("#collect-keyboard").checked,
    collect_app_switches: document.querySelector("#collect-app-switches").checked,
    collect_agents: document.querySelector("#collect-agents").checked,
    animation_intensity: Number(document.querySelector("#animation-intensity").value) / 100,
    lens_intensity: Number(document.querySelector("#lens-intensity").value) / 100,
    performance_mode: document.querySelector("#performance-mode").value,
    quiet_hours: {
      enabled: document.querySelector("#quiet-enabled").checked,
      start: document.querySelector("#quiet-start").value || "22:00",
      end: document.querySelector("#quiet-end").value || "08:00",
    },
    retention_days: Number(document.querySelector("#retention-days").value) || 30,
    launch_at_startup: document.querySelector("#launch-at-startup").checked,
    decorative_shape_tour: document.querySelector("#decorative-shape-tour").checked,
  };
}

function updateRangeOutputs() {
  document.querySelector("#animation-output").value =
    `${document.querySelector("#animation-intensity").value}%`;
  document.querySelector("#lens-output").value =
    `${document.querySelector("#lens-intensity").value}%`;
}

function updateSettingsPreviewPolicy() {
  if (!settingsRendererController || !currentSettings) {
    return;
  }
  const settings = readSettingsForm();
  settingsRendererController.setPolicy(settings.performance_mode, {
    animationIntensity: settings.animation_intensity,
    lensIntensity: settings.lens_intensity,
    decorativeShapeTour: settings.decorative_shape_tour,
  });
}

function ensureSettingsPreviewRenderer() {
  if (settingsRendererPromise) {
    settingsRendererController?.setVisible(true);
    return settingsRendererPromise;
  }
  settingsRendererPromise = window.PressureBlackHole
    .start(
      document.querySelector("#settings-blackhole-preview"),
      () => dashboardPressure,
      {
        // 预览复用同一 Shader 与压力语义，但只在设置页可见时运行，避免增加常驻开销。
        resourceMode: currentSettings?.performance_mode ?? "balanced",
        // 设置卡片只负责预览材质与动态，不承担桌面悬浮窗的抗锯齿质量门禁。
        supersample: 1,
        presentationScale: 1.35,
        animationIntensity: currentSettings?.animation_intensity ?? defaultSettings.animation_intensity,
        lensIntensity: currentSettings?.lens_intensity ?? defaultSettings.lens_intensity,
        decorativeShapeTour: currentSettings?.decorative_shape_tour ?? false,
        readVisualState: () => dashboardVisualState,
      },
    )
    .then((controller) => {
      settingsRendererController = controller;
      settingsPreviewStage.classList.add("is-ready");
      updateSettingsPreviewPolicy();
      controller.setVisible(
        !document.hidden && !document.querySelector("#settings-view").hidden,
      );
      return controller;
    })
    .catch((error) => {
      document.querySelector("#settings-preview-status").textContent =
        `预览不可用：${error.message}`;
      throw error;
    });
  return settingsRendererPromise;
}

function syncRendererVisibility() {
  const pageVisible = !document.hidden;
  const dashboardVisible =
    pageVisible && !document.querySelector("#dashboard-view").hidden;
  const settingsVisible =
    pageVisible && !document.querySelector("#settings-view").hidden;
  rendererController?.setVisible(dashboardVisible);
  if (settingsVisible) {
    ensureSettingsPreviewRenderer().catch(() => {});
  } else {
    settingsRendererController?.setVisible(false);
  }
}

function browserPreviewData() {
  // 浏览器模式只用于端到端和视觉验收；Tauri 始终读取本机真实聚合数据。
  const previewScore = Math.max(
    0,
    Math.min(100, Number(new URLSearchParams(location.search).get("preview")) || 64),
  );
  const previewReasons = previewScore >= 70
    ? [
        "连续活跃 104 分钟，恢复窗口正在变窄",
        "Agent 上下文达到 71%，切换成本开始上升",
        "输入节奏高于今天的个人基线",
      ]
    : previewScore >= 40
      ? [
          "连续活跃时间正在接近今天的高位",
          "Agent 上下文持续增长，需要留意切换成本",
          "输入节奏略高于个人基线",
        ]
      : [
          "输入节奏接近今天的个人基线",
          "应用切换保持在平稳范围",
          "连续活跃时间尚未触发恢复提醒",
        ];
  const now = new Date();
  const snapshot = {
    recorded_at: now.toISOString(),
    pressure: {
      score: previewScore,
      raw_score: previewScore - 2,
      level: previewScore >= 70 ? "high" : previewScore >= 40 ? "elevated" : "calm",
      reasons: previewReasons,
      confidence: 0.82,
      confidence_level: "high",
      calibration_adjustment: 2,
      calibration_reports: 4,
      visual_state: previewScore >= 70 ? "overloaded" : previewScore >= 40 ? "focused" : "calm",
      advice: previewScore >= 70
        ? {
            title: "离开屏幕五分钟",
            detail: "站起来、喝水，让持续注意力真正中断一次。",
            action: "take_break",
            suggested_minutes: 5,
          }
        : null,
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
      agent_metric_quality: "exact",
      active_agents: 3,
      continuous_active_seconds: 6240,
      app_switches: 26,
      window_coverage_seconds: 60,
      collection_paused: false,
    },
    quiet_hours_active: false,
    source_health: [
      { id: "keyboard", label: "键盘节奏", status: "healthy", detail: "只保留滚动聚合计数" },
      { id: "history", label: "本地历史", status: "healthy", detail: "SQLite WAL · 每分钟独立写入" },
      { id: "codex", label: "Codex", status: "healthy", detail: "精确 token_count" },
      { id: "claude", label: "Claude Code", status: "limited", detail: "估算上下文占用" },
    ],
  };
  const history = Array.from({ length: 30 }, (_, index) => {
    const recorded = new Date(now);
    recorded.setMinutes(now.getMinutes() - (29 - index) * 3);
    return {
      recorded_at: recorded.toISOString(),
      score: Math.max(12, Math.min(92, 30 + index * 1.2 + Math.sin(index / 2) * 13)),
    };
  });
  return {
    snapshot,
    settings: structuredClone(defaultSettings),
    history,
    summary: {
      sample_count: history.length,
      covered_minutes: history.length,
      average_score: 54,
      peak_score: 76,
      high_minutes: 4,
      self_report_count: 4,
      headline: "今天午后负荷持续抬升；自评已使模型向你的真实感受校准 2 分。",
    },
  };
}

async function refreshDashboardData() {
  try {
    const data = invoke ? await invoke("get_dashboard_data") : browserPreviewData();
    render(data.snapshot);
    renderHistory(data.history, data.summary);
    applySettingsToForm(data.settings);
  } catch (error) {
    document.querySelector("#level").textContent = `读取本地状态失败：${error}`;
  }
}

async function refreshSnapshot() {
  if (!invoke) {
    render(browserPreviewData().snapshot);
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

document.querySelector(".view-switcher").addEventListener("click", (event) => {
  const button = event.target.closest("button[data-view]");
  if (!button) {
    return;
  }
  document.querySelectorAll(".view-switcher button").forEach((item) => {
    item.classList.toggle("is-active", item === button);
  });
  document.querySelectorAll(".app-view").forEach((view) => {
    const active = view.id === button.dataset.view;
    view.hidden = !active;
    view.classList.toggle("is-active", active);
  });
  syncRendererVisibility();
});

// 窗口最小化或被系统隐藏时暂停两个 WebGL 循环，恢复后只启动当前页面的渲染器。
document.addEventListener("visibilitychange", syncRendererVisibility);

document.querySelector(".preview-background-switcher").addEventListener("click", (event) => {
  const button = event.target.closest("button[data-preview-background]");
  if (!button) {
    return;
  }
  const background = button.dataset.previewBackground;
  const labels = {
    dark: "深色桌面",
    light: "浅色桌面",
    complex: "复杂桌面",
  };
  settingsPreviewStage.dataset.previewBackground = background;
  document.querySelectorAll(".preview-background-switcher button").forEach((item) => {
    item.setAttribute("aria-pressed", String(item === button));
  });
  document.querySelector("#settings-preview-status").textContent =
    `${labels[background]} · 设置会即时反映`;
});

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
  try {
    const snapshot = invoke ? await invoke("record_self_report", { value }) : browserPreviewData().snapshot;
    render(snapshot);
    document.querySelectorAll("#self-report button").forEach((item) => {
      item.setAttribute("aria-pressed", String(item === button));
    });
    document.querySelector("#report-status").textContent = invoke
      ? "已保存到本机；新的个人校准已参与当前评分。"
      : "预览选择已更新；Tauri 运行时会保存到本机。";
    await refreshDashboardData();
  } catch (error) {
    document.querySelector("#report-status").textContent = `自评保存失败：${error}`;
  }
});

document.querySelector("#settings-form").addEventListener("input", () => {
  updateRangeOutputs();
  updateSettingsPreviewPolicy();
  settingsDirty = true;
  document.querySelector("#settings-status").textContent = "有未保存的修改";
});

document.querySelector("#settings-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const status = document.querySelector("#settings-status");
  const settings = readSettingsForm();
  status.textContent = "正在保存…";
  try {
    const snapshot = invoke
      ? await invoke("update_settings", { settings })
      : browserPreviewData().snapshot;
    applySettingsToForm(settings, { force: true });
    render(snapshot);
    status.textContent = invoke ? "已保存到本机并立即生效" : "预览设置已生效";
  } catch (error) {
    status.textContent = `保存失败：${error}`;
  }
});

document.querySelector("#check-update").addEventListener("click", async (event) => {
  const button = event.currentTarget;
  const status = document.querySelector("#update-status");
  if (!invoke) {
    status.textContent = "浏览器预览不连接更新服务器";
    return;
  }
  if (button.dataset.install === "true") {
    button.disabled = true;
    status.textContent = "正在下载并验证签名；Windows 安装前会自动退出…";
    try {
      await invoke("install_update");
    } catch (error) {
      button.disabled = false;
      status.textContent = `安装失败：${error}`;
    }
    return;
  }
  button.disabled = true;
  status.textContent = "正在检查签名更新源…";
  try {
    const update = await invoke("check_for_update");
    if (update) {
      button.dataset.install = "true";
      button.textContent = `安装 ${update.version}`;
      status.textContent = `当前 ${update.current_version}，已发现 ${update.version}`;
    } else {
      status.textContent = "已经是最新版本";
    }
  } catch (error) {
    status.textContent = `检查失败：${error}`;
  } finally {
    button.disabled = false;
  }
});

async function clearHistory(todayOnly) {
  const data = invoke
    ? await invoke("clear_history", { todayOnly })
    : { ...browserPreviewData(), history: [], summary: null };
  renderHistory(data.history, data.summary);
  document.querySelector("#settings-status").textContent =
    todayOnly ? "今天的聚合历史已清除" : "全部聚合历史与自评已清除";
}

document.querySelector("#clear-today").addEventListener("click", () => {
  clearHistory(true).catch((error) => {
    document.querySelector("#settings-status").textContent = `清除失败：${error}`;
  });
});

document.querySelector("#clear-all").addEventListener("click", (event) => {
  const button = event.currentTarget;
  if (button.dataset.armed !== "true") {
    button.dataset.armed = "true";
    button.textContent = "再次点击确认清除";
    window.setTimeout(() => {
      button.dataset.armed = "false";
      button.textContent = "清除全部历史";
    }, 5000);
    return;
  }
  button.dataset.armed = "false";
  button.textContent = "清除全部历史";
  clearHistory(false).catch((error) => {
    document.querySelector("#settings-status").textContent = `清除失败：${error}`;
  });
});

document.querySelector("#start-recovery").addEventListener("click", (event) => {
  const button = event.currentTarget;
  const totalSeconds = Math.max(1, Number(button.dataset.minutes) || 2) * 60;
  let remaining = totalSeconds;
  clearInterval(recoveryTimer);
  button.disabled = true;
  const update = () => {
    const minutes = String(Math.floor(remaining / 60)).padStart(2, "0");
    const seconds = String(remaining % 60).padStart(2, "0");
    button.textContent = `恢复中 ${minutes}:${seconds}`;
    if (remaining <= 0) {
      clearInterval(recoveryTimer);
      button.disabled = false;
      button.textContent = "恢复完成 · 再看一次";
    }
    remaining -= 1;
  };
  update();
  recoveryTimer = window.setInterval(update, 1000);
});

async function initializeEvents() {
  if (!listen) {
    return;
  }
  await listen("snapshot-updated", (event) => render(event.payload));
  await listen("settings-updated", (event) => applySettingsToForm(event.payload, { force: true }));
  await listen("update-progress", (event) => {
    const status = document.querySelector("#update-status");
    if (event.payload.event === "started") {
      status.textContent = "更新包下载中…";
    } else if (event.payload.event === "finished") {
      status.textContent = "更新已验证并安装";
    }
  });
}

refreshDashboardData();
refreshOverlayButton();
initializeEvents().catch(() => {});
// 事件是主通道；低频轮询只用于 WebView 休眠恢复后的自愈。
window.setInterval(refreshSnapshot, 10_000);
window.setInterval(refreshDashboardData, 60_000);

window.PressureBlackHole
  .start(
    document.querySelector("#dashboard-blackhole"),
    () => dashboardPressure,
    {
      // 超采样仍受性能档位的 DPR 与光线步数上限约束，避免设置形同虚设。
      resourceMode: "balanced",
      supersample: 1.5,
      animationIntensity: defaultSettings.animation_intensity,
      lensIntensity: 0,
      shapeOverride,
      readVisualState: () => dashboardVisualState,
    },
  )
  .then((controller) => {
    rendererController = controller;
    orbit.classList.add("is-ready");
    syncRendererVisibility();
    if (currentSettings) {
      applySettingsToForm(currentSettings);
    }
  })
  .catch((error) => {
    visualStatus.textContent = `引力模型不可用：${error.message}`;
  });

window.addEventListener("beforeunload", () => {
  clearInterval(recoveryTimer);
  rendererController?.dispose();
  settingsRendererController?.dispose();
});
