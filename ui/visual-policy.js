((root, factory) => {
  const api = factory();
  if (typeof module === "object" && module.exports) {
    module.exports = api;
  }
  root.PressureVisuals = api;
})(globalThis, () => {
  const families = Object.freeze({
    // 第一版 Inferno 是所有未知状态和低压力状态的视觉锚点，避免启动成正视圆环。
    calm: Object.freeze([0, 2]),
    focused: Object.freeze([1, 0]),
    overloaded: Object.freeze([4, 5, 0]),
    uncertain: Object.freeze([0]),
  });
  const visibleTour = Object.freeze([0, 1, 2, 3, 4, 5]);

  function familyFor(semanticState) {
    return families[String(semanticState)] ?? families.uncertain;
  }

  function primaryShape(semanticState) {
    return familyFor(semanticState)[0];
  }

  function tourFor(semanticState) {
    const primary = primaryShape(semanticState);
    return Object.freeze([
      primary,
      ...visibleTour.filter((shape) => shape !== primary),
    ]);
  }

  function tourSlot(elapsedSeconds, familyLength, secondsPerShape = 10) {
    const length = Math.max(1, Math.floor(Number(familyLength) || 1));
    const interval = Math.max(1, Number(secondsPerShape) || 10);
    return Math.floor(Math.max(0, Number(elapsedSeconds) || 0) / interval) % length;
  }

  function visualPressure(pressure) {
    const normalizedPressure = Math.max(0, Math.min(1, Number(pressure) || 0));
    // 低压区共用同一视觉基线，过滤正常工作中的小幅评分抖动。
    return normalizedPressure <= 0.25
      ? 0
      : (normalizedPressure - 0.25) / 0.75;
  }

  function motionScale(reduceMotion, pressure = 1) {
    const pressureScale = 0.45 + visualPressure(pressure) * 0.55;
    // 低压保持可辨认的盘面流动；系统减少动画只继续降速，不把时间相位冻结。
    return pressureScale * (reduceMotion ? 0.18 : 1);
  }

  function createPressureStateController(initialState = "calm") {
    let state = initialState === "focused" || initialState === "overloaded"
      ? initialState
      : "calm";
    let pendingSince = null;
    let pendingTarget = null;

    return Object.freeze({
      update(pressure, timestampMilliseconds = Date.now()) {
        const normalizedPressure = Math.max(0, Math.min(1, Number(pressure) || 0));
        const timestamp = Number(timestampMilliseconds) || 0;
        let target = null;
        let confirmationMilliseconds = 0;
        if (state === "calm" && normalizedPressure >= 0.45) {
          target = "focused";
          confirmationMilliseconds = 20_000;
        } else if (state === "focused" && normalizedPressure >= 0.75) {
          target = "overloaded";
          confirmationMilliseconds = 15_000;
        } else if (state === "focused" && normalizedPressure <= 0.35) {
          target = "calm";
          confirmationMilliseconds = 45_000;
        } else if (state === "overloaded" && normalizedPressure <= 0.65) {
          target = "focused";
          confirmationMilliseconds = 60_000;
        }
        if (!target) {
          pendingSince = null;
          pendingTarget = null;
          return state;
        }
        // 达到阈值只开始计时，避免单次尖峰立刻改变黑洞语义。
        if (pendingTarget !== target) {
          pendingTarget = target;
          pendingSince = timestamp;
        }
        if (timestamp - pendingSince >= confirmationMilliseconds) {
          state = target;
          pendingSince = null;
          pendingTarget = null;
        }
        return state;
      },
    });
  }

  return Object.freeze({
    // 静止悬浮时恢复局部桌面折射；拖动生命周期会另行暂停采样。
    desktopRefractionEnabled: true,
    createPressureStateController,
    familyFor,
    motionScale,
    primaryShape,
    tourFor,
    tourSlot,
    visualPressure,
  });
});
