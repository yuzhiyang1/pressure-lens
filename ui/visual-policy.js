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

  function familyFor(semanticState) {
    return families[String(semanticState)] ?? families.uncertain;
  }

  function primaryShape(semanticState) {
    return familyFor(semanticState)[0];
  }

  function motionScale(reduceMotion) {
    // 尊重系统偏好，但保留足以表达“活着”的低速运动。
    return reduceMotion ? 0.18 : 1;
  }

  return Object.freeze({
    // 正常悬浮启用桌面折射；拖动期间由渲染器生命周期单独暂停。
    desktopRefractionEnabled: true,
    familyFor,
    motionScale,
    primaryShape,
  });
});
