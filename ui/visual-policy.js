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

  function motionScale(reduceMotion) {
    // 尊重系统偏好，但保留足以表达“活着”的低速运动。
    return reduceMotion ? 0.18 : 1;
  }

  return Object.freeze({
    // 保持第一版黑洞本体的完整流光，不叠加会压暗盘面的桌面折射蒙层。
    desktopRefractionEnabled: false,
    familyFor,
    motionScale,
    primaryShape,
    tourFor,
    tourSlot,
  });
});
