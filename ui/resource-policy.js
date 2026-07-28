(() => {
  const clamp = (value, minimum, maximum) =>
    Math.max(minimum, Math.min(maximum, Number(value) || 0));

  const tiers = Object.freeze({
    eco: Object.freeze({
      framesPerSecond: 12,
      maximumDpr: 1,
      raySteps: 28,
      backdropFramesPerSecond: 0.25,
    }),
    balanced: Object.freeze({
      framesPerSecond: 20,
      maximumDpr: 1.5,
      raySteps: 40,
      backdropFramesPerSecond: 1,
    }),
    vivid: Object.freeze({
      framesPerSecond: 30,
      maximumDpr: 1.75,
      raySteps: 52,
      backdropFramesPerSecond: 3,
    }),
  });

  function resolve(mode = "balanced", intensities = {}) {
    const tier = tiers[mode] ?? tiers.balanced;
    return Object.freeze({
      mode: tiers[mode] ? mode : "balanced",
      ...tier,
      animationIntensity: clamp(intensities.animationIntensity ?? 0.75, 0, 1),
      lensIntensity: clamp(intensities.lensIntensity ?? 0.65, 0, 1),
      // 形态巡游是明确的用户偏好；默认关闭以保持“一个形态代表一种状态”。
      decorativeShapeTour: Boolean(intensities.decorativeShapeTour),
    });
  }

  function captureEnabled(policy, lifecycle = {}) {
    return Boolean(
      policy
      && policy.lensIntensity > 0
      && lifecycle.visible !== false
      && lifecycle.paused !== true,
    );
  }

  window.PressureResources = Object.freeze({
    resolve,
    captureEnabled,
    tiers,
  });
})();
