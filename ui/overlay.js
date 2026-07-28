const invoke = window.__TAURI__?.core?.invoke;
const listen = window.__TAURI__?.event?.listen;
const canvas = document.querySelector("#blackhole-canvas");
const lens = document.querySelector("#desktop-lens");
const moveHint = document.querySelector("#move-hint");
const gravityLock = document.querySelector("#gravity-lock");
const gravityLockTitle = document.querySelector("#gravity-lock-title");
const gravityLockDetail = document.querySelector("#gravity-lock-detail");
const statusPill = document.querySelector(".status");
const urlParameters = new URLSearchParams(location.search);
const shapeParameter = urlParameters.get("shape");
const shapeOverride = shapeParameter == null
  ? null
  : Math.max(0, Math.min(6.999, Number(shapeParameter) || 0));
const rotationParameter = urlParameters.get("rotation");
const rotationOverride = rotationParameter == null
  ? null
  : (Number(rotationParameter) || 0) * Math.PI / 180;
const timeParameter = urlParameters.get("time");
const timeOverride = timeParameter == null
  ? null
  : Math.max(0, Number(timeParameter) || 0);
const lensStatus = document.querySelector("#overlay-lens-status");

const labels = {
  calm: "负荷平稳",
  elevated: "负荷升高",
  high: "建议主动降载",
};

let targetPressure = 0;
let targetVisualState = "uncertain";
let previewBackdropPayload;
let moveModeEnabled = false;
let rendererController = null;
let hoverPreparedForDrag = false;
let statusIdleTimer = null;
let lastDisplayedScore = null;
let currentSettings = {
  performance_mode: "balanced",
  animation_intensity: 0.65,
  lens_intensity: 0.55,
  decorative_shape_tour: false,
};
// 静止悬浮时开启局部折射；拖动前由渲染器清空旧坐标纹理并暂停采样。
const desktopRefractionEnabled = window.PressureVisuals.desktopRefractionEnabled;

function revealStatus({ persistent = false } = {}) {
  if (statusIdleTimer !== null) {
    window.clearTimeout(statusIdleTimer);
    statusIdleTimer = null;
  }
  statusPill.classList.remove("is-idle");
  if (persistent) {
    return;
  }
  // 常驻工具在稳定状态下退到环境层；变化或悬停时再主动出现。
  statusIdleTimer = window.setTimeout(() => {
    if (!lens.classList.contains("is-hovering") && !lens.classList.contains("is-dragging")) {
      statusPill.classList.add("is-idle");
    }
  }, 4_000);
}

function applyRendererSettings(settings) {
  currentSettings = { ...currentSettings, ...settings };
  rendererController?.setPolicy(currentSettings.performance_mode, {
    animationIntensity: currentSettings.animation_intensity,
    lensIntensity: desktopRefractionEnabled ? currentSettings.lens_intensity : 0,
    decorativeShapeTour: currentSettings.decorative_shape_tour,
  });
}

function updateHoverProgress(payload = {}) {
  const progress = Math.max(0, Math.min(1, Number(payload.progress) || 0));
  const ready = Boolean(payload.ready);
  lens.style.setProperty("--hover-progress", progress.toFixed(3));
  lens.classList.toggle("is-hovering", progress > 0);
  lens.classList.toggle("is-hover-ready", ready);
  gravityLock.setAttribute("aria-hidden", String(progress <= 0));
  if (progress > 0) {
    revealStatus({ persistent: true });
  } else {
    revealStatus();
  }

  if (ready) {
    if (!hoverPreparedForDrag) {
      hoverPreparedForDrag = true;
      // 两秒悬停已经给足淡出时间；真正按下时桌面纹理早已清空。
      rendererController?.prepareForDrag();
    }
    gravityLockTitle.textContent = "引力锚定";
    gravityLockDetail.textContent = "按住黑洞即可拖动";
    return;
  }

  if (hoverPreparedForDrag && !lens.classList.contains("is-dragging")) {
    hoverPreparedForDrag = false;
    rendererController?.resumeAfterDrag();
  }

  const secondsLeft = Math.ceil((2 - progress * 2) * 10) / 10;
  gravityLockTitle.textContent = "引力感应";
  gravityLockDetail.textContent = `保持悬停 ${secondsLeft.toFixed(1)}s`;
}

function updateMoveMode(enabled) {
  moveModeEnabled = Boolean(enabled);
  lens.classList.toggle("is-move-mode", moveModeEnabled);
  moveHint.setAttribute("aria-hidden", String(!moveModeEnabled));
}

async function initializeMoveMode() {
  if (!invoke) {
    if (urlParameters.has("hover")) {
      const previewProgress = Math.max(
        0,
        Math.min(1, Number(urlParameters.get("hover")) || 0),
      );
      updateHoverProgress({
        progress: previewProgress,
        ready: previewProgress >= 1,
      });
    }
    return;
  }

  // Rust 端负责真正切换点击穿透；前端只同步光标和解锁提示。
  updateMoveMode(await invoke("get_overlay_move_mode"));
  if (listen) {
    await listen("overlay-move-mode", (event) => updateMoveMode(event.payload));
    await listen("overlay-hover-progress", (event) => updateHoverProgress(event.payload));
    await listen("overlay-visibility", (event) => {
      rendererController?.setVisible(Boolean(event.payload));
    });
    await listen("settings-updated", (event) => applyRendererSettings(event.payload));
    await listen("snapshot-updated", (event) => renderOverlaySnapshot(event.payload));
  }
}

lens.addEventListener("pointerdown", async (event) => {
  if (!moveModeEnabled || event.button !== 0 || !invoke) {
    return;
  }

  event.preventDefault();
  lensStatus.textContent = "移动中 · 折射暂停";
  lensStatus.classList.remove("is-active");
  lens.classList.add("is-dragging");
  revealStatus({ persistent: true });
  try {
    await rendererController?.prepareForDrag();
    // 交给系统窗口管理器拖动，跨显示器和 DPI 缩放时比手算坐标更可靠。
    await invoke("start_overlay_dragging");
  } finally {
    // 松手后立即恢复鼠标穿透；再次移动前必须先移出黑洞再重新悬停。
    await invoke("finish_overlay_dragging").catch(() => {});
    lensStatus.textContent = "折射恢复中";
    hoverPreparedForDrag = false;
    await rendererController?.resumeAfterDrag();
    updateHoverProgress();
    lens.classList.remove("is-dragging");
  }
});

function createPreviewBackdrop() {
  if (previewBackdropPayload) {
    return previewBackdropPayload;
  }

  // 浏览器验收使用合成桌面，不读取真实屏幕，便于观察网格和窗口边缘是否被折射。
  const preview = document.createElement("canvas");
  preview.width = 420;
  preview.height = 420;
  const context = preview.getContext("2d", { willReadFrequently: true });
  const gradient = context.createLinearGradient(0, 0, 420, 420);
  gradient.addColorStop(0, "#e7eef8");
  gradient.addColorStop(1, "#9bb4d2");
  context.fillStyle = gradient;
  context.fillRect(0, 0, 420, 420);
  context.strokeStyle = "rgba(40, 68, 104, .28)";
  context.lineWidth = 2;
  for (let offset = 0; offset <= 420; offset += 36) {
    context.beginPath();
    context.moveTo(offset, 0);
    context.lineTo(offset, 420);
    context.moveTo(0, offset);
    context.lineTo(420, offset);
    context.stroke();
  }
  context.fillStyle = "rgba(252, 253, 255, .92)";
  context.fillRect(46, 62, 328, 236);
  context.strokeStyle = "#557395";
  context.strokeRect(46, 62, 328, 236);
  context.fillStyle = "#35506d";
  context.font = "600 22px system-ui";
  context.fillText("DESKTOP LENS", 72, 110);
  context.font = "15px system-ui";
  context.fillText("这些直线会在事件视界周围弯曲", 72, 142);

  const pixels = context.getImageData(0, 0, preview.width, preview.height).data;
  previewBackdropPayload = new Uint8Array(8 + pixels.byteLength);
  const header = new DataView(previewBackdropPayload.buffer, 0, 8);
  header.setUint32(0, preview.width, true);
  header.setUint32(4, preview.height, true);
  previewBackdropPayload.set(pixels, 8);
  return previewBackdropPayload;
}

async function refreshOverlay() {
  if (!invoke) {
    // 浏览器视觉验收可用 ?preview=0.8，或用 ?backdrop=0.8 同时开启合成桌面。
    const previewValue = urlParameters.get("preview") ?? urlParameters.get("backdrop");
    targetPressure = Math.max(
      0,
      Math.min(1, Number(previewValue) || 0),
    );
    targetVisualState =
      targetPressure >= .7 ? "overloaded" : targetPressure >= .4 ? "focused" : "calm";
    document.querySelector("#overlay-score").textContent = Math.round(targetPressure * 100);
    document.querySelector("#overlay-label").textContent =
      targetPressure >= .7
        ? labels.high
        : targetPressure >= .4
          ? labels.elevated
          : labels.calm;
    lastDisplayedScore = Math.round(targetPressure * 100);
    revealStatus();
    return;
  }

  try {
    const snapshot = await invoke("get_snapshot");
    renderOverlaySnapshot(snapshot);
  } catch {
    document.querySelector("#overlay-label").textContent = "状态暂不可用";
  }
}

function renderOverlaySnapshot(snapshot) {
  targetPressure = snapshot.pressure.score / 100;
  targetVisualState = snapshot.pressure.visual_state ?? "uncertain";
  const displayedScore = Math.round(snapshot.pressure.score);
  document.querySelector("#overlay-score").textContent = displayedScore;
  document.querySelector("#overlay-label").textContent = snapshot.sample.collection_paused
    ? "采集已暂停"
    : snapshot.quiet_hours_active
      ? "安静时段"
      : labels[snapshot.pressure.level] ?? "建立基线";
  // “暂停全部采集”同样停止桌面截图；悬浮窗只保留最后一帧，不再读取桌面。
  rendererController?.setPaused(Boolean(
    snapshot.quiet_hours_active || snapshot.sample.collection_paused,
  ));
  if (displayedScore !== lastDisplayedScore) {
    lastDisplayedScore = displayedScore;
    revealStatus();
  }
}

async function initializeSettings() {
  if (!invoke) {
    return;
  }
  applyRendererSettings(await invoke("get_settings"));
}

initializeMoveMode().catch(() => updateMoveMode(false));
initializeSettings().catch(() => {});
refreshOverlay();
// snapshot-updated 是主通道；低频轮询只负责 WebView 休眠恢复后的自愈。
setInterval(refreshOverlay, 10_000);
window.PressureBlackHole
  .start(canvas, () => targetPressure, {
    resourceMode: "balanced",
    // 保留超采样意图，但最终 DPR、光线步数和帧率都由用户选择的性能档封顶。
    supersample: 1.75,
    animationIntensity: currentSettings.animation_intensity,
    lensIntensity: desktopRefractionEnabled ? currentSettings.lens_intensity : 0,
    decorativeShapeTour: currentSettings.decorative_shape_tour,
    readVisualState: () => targetVisualState,
    // 桌面静止时采样当前位置；拖动生命周期由控制器暂停并丢弃所有旧坐标帧。
    readBackdrop: desktopRefractionEnabled
      ? invoke
        ? async () => invoke("capture_overlay_background")
        : urlParameters.has("backdrop")
          ? async () => createPreviewBackdrop()
          : undefined
      : undefined,
    onBackdropReady: (diagnostics) => {
      lensStatus.textContent = "桌面折射";
      lensStatus.setAttribute(
        "aria-label",
        `桌面折射已启用，帧尺寸 ${diagnostics.width}×${diagnostics.height}，亮度变化 ${diagnostics.brightnessRange}`,
      );
      lensStatus.classList.add("is-active");
    },
    onBackdropError: () => {
      lensStatus.textContent = "折射暂不可用";
      lensStatus.classList.remove("is-active");
    },
    shapeOverride,
    // 仅浏览器视觉验收使用，例如 ?rotation=90 固定最容易混叠的角度。
    rotationOverride,
    timeOverride,
  })
  .then(async (controller) => {
    rendererController = controller;
    // 暴露只读验收入口，浏览器测试可验证真实 WebGL 折射生命周期。
    window.PressureOverlayPreview = Object.freeze({ controller });
    if (invoke) {
      controller.setVisible(await invoke("get_overlay_visible").catch(() => true));
    } else if (!urlParameters.has("backdrop")) {
      // 普通浏览器预览没有桌面采集通道，不把实现状态暴露给视觉验收用户。
      lensStatus.textContent = "浏览器视觉预览";
      lensStatus.classList.add("is-active");
    }
    applyRendererSettings(currentSettings);
    if (hoverPreparedForDrag) {
      await controller.prepareForDrag();
    }
    lens.classList.add("is-ready");
    revealStatus();
  })
  .catch((error) => {
    document.querySelector("#overlay-label").textContent = `渲染失败：${error.message}`;
  });

window.addEventListener("beforeunload", () => rendererController?.dispose());
