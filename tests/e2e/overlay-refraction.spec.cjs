const { test, expect } = require("@playwright/test");

// CI 使用 SwiftShader 软件渲染，小视口仍能验证真实 WebGL2 折射生命周期，
// 同时避免高分辨率光线积分阻塞浏览器主线程。
test.use({ viewport: { width: 360, height: 360 } });

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

  const pixels = await page.locator("#blackhole-canvas").evaluate(async (canvas) => {
    await new Promise((resolve) => requestAnimationFrame(() => resolve()));
    const gl = canvas.getContext("webgl2");
    const dpr = canvas.width / canvas.clientWidth;
    const readAlpha = (cssX, cssY) => {
      const pixel = new Uint8Array(4);
      gl.readPixels(
        Math.round(cssX * dpr),
        Math.round((canvas.clientHeight - cssY) * dpr),
        1,
        1,
        gl.RGBA,
        gl.UNSIGNED_BYTE,
        pixel,
      );
      return pixel[3];
    };
    const centerX = canvas.clientWidth / 2;
    const centerY = canvas.clientHeight / 2;
    let lensAlpha = 0;
    // 黑洞中心本来就是透明/纯黑区域；在事件视界外的环带取最大值，
    // 才能稳定验证局部折射确实存在，而不依赖某一种旋转角度。
    for (let radius = 48; radius <= 118; radius += 10) {
      for (let angle = 0; angle < Math.PI * 2; angle += Math.PI / 12) {
        lensAlpha = Math.max(
          lensAlpha,
          readAlpha(
            centerX + Math.cos(angle) * radius,
            centerY + Math.sin(angle) * radius,
          ),
        );
      }
    }

    return {
      safeEdgeAlpha: readAlpha(34, canvas.clientHeight / 2),
      lensAlpha,
    };
  });

  expect(pixels.safeEdgeAlpha).toBeLessThan(12);
  expect(pixels.lensAlpha).toBeGreaterThan(40);
});

test("稳定低压时状态胶囊退到环境层", async ({ page }) => {
  await page.clock.install();
  await page.goto("/overlay.html?preview=0.13");

  const status = page.locator(".status");
  await expect(status).not.toHaveClass(/is-idle/);
  await page.clock.fastForward(4_100);
  await expect(status).toHaveClass(/is-idle/);
});
