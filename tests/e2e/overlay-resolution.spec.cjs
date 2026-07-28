const { test, expect } = require("@playwright/test");

test.beforeEach(async ({ page }) => {
  // 这里只验证真实 Shader、DPR、积分步数与形态语义，不做像素级视觉比较。
  // 小画布让 GitHub Runner 的 SwiftShader 也能在门禁时间内完成第一帧。
  await page.setViewportSize({ width: 240, height: 180 });
});

test("13 分桌面黑洞保持低压形态并使用平衡档渲染预算", async ({ page }) => {
  await page.goto("/overlay.html?preview=0.13");

  await expect.poll(async () => (
    page.locator("#blackhole-canvas").evaluate((canvas) => ({
      dpr: canvas.width / canvas.clientWidth,
      raySteps: Number(canvas.dataset.raySteps),
      framesPerSecond: Number(canvas.dataset.framesPerSecond),
      tourEnabled: canvas.dataset.tourEnabled,
      tourSize: Number(canvas.dataset.tourSize),
      shapeTo: Number(canvas.dataset.shapeTo),
    }))
  ), { timeout: 20_000 }).toEqual({
    dpr: 1.75,
    raySteps: 52,
    framesPerSecond: 15,
    tourEnabled: "false",
    tourSize: 2,
    shapeTo: 0,
  });
});

test("压力升到 78 分后切换为与仪表盘一致的过载形态", async ({ page }) => {
  await page.goto("/overlay.html?preview=0.78");

  await expect.poll(async () => (
    page.locator("#blackhole-canvas").evaluate((canvas) => ({
      shapeTo: Number(canvas.dataset.shapeTo),
      tourEnabled: canvas.dataset.tourEnabled,
    }))
  )).toEqual({
    shapeTo: 4,
    tourEnabled: "false",
  });
});
