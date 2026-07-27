async (page) => {
  const result = await page.evaluate(async () => {
    const makeFrame = (red, green, blue) => {
      const payload = new Uint8Array(12);
      const header = new DataView(payload.buffer);
      header.setUint32(0, 1, true);
      header.setUint32(4, 1, true);
      payload.set([red, green, blue, 255], 8);
      return payload;
    };

    let releaseStaleFrame;
    const staleFrame = new Promise((resolve) => {
      releaseStaleFrame = resolve;
    });
    let captureCalls = 0;
    const canvas = document.createElement("canvas");
    canvas.style.width = "16px";
    canvas.style.height = "16px";
    document.body.appendChild(canvas);

    const controller = await window.PressureBlackHole.start(canvas, () => 0.2, {
      framesPerSecond: 30,
      backdropFramesPerSecond: 12,
      readBackdrop: () => {
        captureCalls += 1;
        return captureCalls === 1
          ? staleFrame
          : Promise.resolve(makeFrame(20, 120, 220));
      },
    });

    await new Promise((resolve) => setTimeout(resolve, 40));
    controller.suspendBackdrop();
    // 模拟拖动前旧坐标截图在暂停后才返回。
    releaseStaleFrame(makeFrame(220, 40, 20));
    await new Promise((resolve) => setTimeout(resolve, 40));
    const callsWhileSuspended = captureCalls;

    controller.resumeBackdrop();
    await new Promise((resolve) => setTimeout(resolve, 80));
    const callsAfterResume = captureCalls;
    canvas.remove();

    return { callsWhileSuspended, callsAfterResume };
  });

  if (result.callsWhileSuspended !== 1 || result.callsAfterResume < 2) {
    throw new Error(`桌面采样生命周期异常：${JSON.stringify(result)}`);
  }
  return result;
}
