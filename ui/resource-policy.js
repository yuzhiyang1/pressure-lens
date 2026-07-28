(() => {
  const clamp = (value, minimum, maximum) =>
    Math.max(minimum, Math.min(maximum, Number(value) || 0));

  const tiers = Object.freeze({
    eco: Object.freeze({
      // 省电档降低刷新频率，但保留足够采样，避免重新出现明显锯齿。
      framesPerSecond: 10,
      maximumDpr: 1.5,
      raySteps: 44,
      backdropFramesPerSecond: 0.25,
    }),
    balanced: Object.freeze({
      framesPerSecond: 15,
      maximumDpr: 2,
      raySteps: 52,
      backdropFramesPerSecond: 1,
    }),
    vivid: Object.freeze({
      framesPerSecond: 30,
      // 鲜明档提供完整积分和双倍采样，供用户主动选择。
      maximumDpr: 2.5,
      raySteps: 56,
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
      // 默认由压力区间决定形态；巡游仅作为用户主动开启的视觉演示。
      decorativeShapeTour: Boolean(intensities.decorativeShapeTour ?? false),
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
