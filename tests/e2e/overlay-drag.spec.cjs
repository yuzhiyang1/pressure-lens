const { test, expect } = require("@playwright/test");

test("开始原生拖动前必须呈现一帧不含旧桌面纹理的黑洞", async ({ page }) => {
  await page.goto("/overlay.html?preview=0.2");

  const result = await page.evaluate(async () => {
    const size = 24;
    const payload = new Uint8Array(8 + size * size * 4);
    const header = new DataView(payload.buffer);
    header.setUint32(0, size, true);
    header.setUint32(4, size, true);
    for (let offset = 8; offset < payload.length; offset += 4) {
      // 使用纯绿色桌面帧，让测试可以从最终画布读取出折射背景是否仍然存在。
      payload.set([0, 255, 0, 255], offset);
    }

    const canvas = document.createElement("canvas");
    canvas.style.width = "96px";
    canvas.style.height = "96px";
    document.body.appendChild(canvas);

    let markBackdropReady;
    const backdropReady = new Promise((resolve) => {
      markBackdropReady = resolve;
    });
    const controller = await window.PressureBlackHole.start(canvas, () => 0.2, {
      resourceMode: "vivid",
      lensIntensity: 1,
      readVisualState: () => "uncertain",
      // Pure lens 没有暖色吸积盘，绿色变化只来自桌面纹理，避免视觉策略影响生命周期断言。
      shapeOverride: 6,
      readBackdrop: async () => payload,
      onBackdropReady: markBackdropReady,
    });

    const readGreen = () => {
      const copy = document.createElement("canvas");
      copy.width = canvas.width;
      copy.height = canvas.height;
      const context = copy.getContext("2d");
      context.drawImage(canvas, 0, 0);
      const pixels = context.getImageData(0, 0, copy.width, copy.height).data;
      let green = 0;
      for (let offset = 1; offset < pixels.length; offset += 4) {
        green += pixels[offset];
      }
      return green;
    };

    await Promise.race([
      backdropReady,
      new Promise((_, reject) => setTimeout(
        () => reject(new Error("桌面纹理未按时进入画布")),
        2_000,
      )),
    ]);
    await new Promise((resolve) => requestAnimationFrame(resolve));
    const greenBeforeDrag = readGreen();

    // 这个 Promise 返回时，调用方将立即进入 Windows 原生拖动循环。
    await controller.prepareForDrag();
    const greenWhenDragStarts = readGreen();
    const diagnostics = controller.getDiagnostics();
    controller.dispose();
    canvas.remove();

    return { greenBeforeDrag, greenWhenDragStarts, diagnostics };
  });

  expect(result.greenBeforeDrag).toBeGreaterThan(0);
  expect(result.greenWhenDragStarts).toBeLessThan(result.greenBeforeDrag * 0.05);
  expect(result.diagnostics.dragPresentationBarriers).toBe(1);
});
