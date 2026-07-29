const { test, expect } = require("@playwright/test");

// CI 使用 SwiftShader 软件渲染，小视口仍能验证真实 WebGL2 折射生命周期，
// 同时避免高分辨率光线积分阻塞浏览器主线程。
test.use({ viewport: { width: 360, height: 360 } });

test("桌面单帧折射在静止期间不会周期重抓，落位后只刷新一次", async ({ page }) => {
  await page.goto("/overlay.html?preview=0.2");

  const captures = await page.evaluate(async () => {
    const makeFrame = () => {
      const payload = new Uint8Array(12);
      const header = new DataView(payload.buffer);
      header.setUint32(0, 1, true);
      header.setUint32(4, 1, true);
      payload.set([42, 88, 132, 255], 8);
      return payload;
    };
    const canvas = document.createElement("canvas");
    canvas.style.width = "16px";
    canvas.style.height = "16px";
    document.body.appendChild(canvas);

    let captureCalls = 0;
    const controller = await window.PressureBlackHole.start(canvas, () => 0.2, {
      resourceMode: "vivid",
      continuousBackdropCapture: false,
      readBackdrop: async () => {
        captureCalls += 1;
        return makeFrame();
      },
    });

    // vivid 档原本约每 333ms 重抓一次；等待 1.1s 足以稳定暴露周期刷新。
    await new Promise((resolve) => setTimeout(resolve, 1_100));
    const whileStationary = captureCalls;

    await controller.prepareForDrag();
    await controller.resumeAfterDrag();
    await new Promise((resolve) => setTimeout(resolve, 500));
    const afterRelocation = captureCalls;

    controller.dispose();
    canvas.remove();
    return { whileStationary, afterRelocation };
  });

  expect(captures.whileStationary).toBe(1);
  expect(captures.afterRelocation).toBe(2);
});

test("静止时开启折射，拖动时关闭，落位后只从新位置恢复", async ({ page }) => {
  await page.goto("/overlay.html?backdrop=0.2");
  await expect(page.locator("#overlay-lens-status")).toHaveText("桌面折射");

  const states = await page.evaluate(async () => {
    const controller = window.PressureOverlayPreview?.controller;
    if (!controller) {
      throw new Error("浏览器验收控制器未就绪");
    }

    const before = controller.getDiagnostics();
    await controller.prepareForDrag();
    const dragging = controller.getDiagnostics();
    await controller.resumeAfterDrag();
    await new Promise((resolve) => setTimeout(resolve, 260));
    const restored = controller.getDiagnostics();

    return { before, dragging, restored };
  });

  expect(states.before.backdropVisibility).toBeGreaterThan(0);
  expect(states.dragging.backdropSuspended).toBe(true);
  expect(states.dragging.backdropVisibility).toBe(0);
  expect(states.restored.backdropSuspended).toBe(false);
  expect(states.restored.captureRequests).toBeGreaterThan(
    states.dragging.captureRequests,
  );
});

test("局部折射不会触碰悬浮窗口边界", async ({ page }) => {
  await page.goto("/overlay.html?backdrop=0.78&shape=6&time=2");
  await expect(page.locator("#overlay-lens-status")).toHaveText("桌面折射");
  await expect.poll(async () => page.evaluate(() => (
    window.PressureOverlayPreview.controller.getDiagnostics().backdropVisibility
  )), { timeout: 20_000 }).toBeGreaterThan(0.5);

  // 默认 WebGL 后备缓冲在合成后允许被浏览器清空。读取最终 canvas 截图，
  // 验证的是用户真正看到的合成像素，也不会依赖 preserveDrawingBuffer。
  const canvasPng = await page.locator("#blackhole-canvas").screenshot();
  const pixels = await page.evaluate(async (pngBase64) => {
    const response = await fetch(`data:image/png;base64,${pngBase64}`);
    const bitmap = await createImageBitmap(await response.blob());
    const surface = new OffscreenCanvas(bitmap.width, bitmap.height);
    const context = surface.getContext("2d");
    context.drawImage(bitmap, 0, 0);
    bitmap.close();
    const rgba = context.getImageData(0, 0, surface.width, surface.height).data;
    const readBrightness = (x, y) => {
      const offset = (Math.round(y) * surface.width + Math.round(x)) * 4;
      return Math.max(rgba[offset], rgba[offset + 1], rgba[offset + 2]);
    };
    const scale = surface.width / 360;
    const centerX = surface.width / 2;
    const centerY = surface.height / 2;
    let lensBrightness = 0;
    // 黑洞中心本来就是透明/纯黑区域；在事件视界外的环带取最大值，
    // 才能稳定验证局部折射确实存在，而不依赖某一种旋转角度。
    for (let radius = 48; radius <= 118; radius += 10) {
      for (let angle = 0; angle < Math.PI * 2; angle += Math.PI / 12) {
        lensBrightness = Math.max(
          lensBrightness,
          readBrightness(
            centerX + Math.cos(angle) * radius * scale,
            centerY + Math.sin(angle) * radius * scale,
          ),
        );
      }
    }

    return {
      safeEdgeBrightness: readBrightness(34 * scale, centerY),
      lensBrightness,
    };
  }, canvasPng.toString("base64"));

  expect(pixels.safeEdgeBrightness).toBeLessThan(28);
  expect(pixels.lensBrightness).toBeGreaterThan(40);
});

test("稳定低压时状态胶囊退到环境层", async ({ page }) => {
  await page.clock.install();
  await page.goto("/overlay.html?preview=0.13");

  const status = page.locator(".status");
  await expect(status).not.toHaveClass(/is-idle/);
  await page.clock.fastForward(4_100);
  await expect(status).toHaveClass(/is-idle/);
});
