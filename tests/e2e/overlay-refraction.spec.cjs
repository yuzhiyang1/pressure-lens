const { test, expect } = require("@playwright/test");

test("悬浮时开启折射，拖动时关闭，松手后从新位置恢复", async ({ page }) => {
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
    await new Promise((resolve) => setTimeout(resolve, 120));
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
