const { test, expect } = require("@playwright/test");

test("桌面黑洞恢复第一版的超采样与完整光线步数", async ({ page }) => {
  await page.goto("/overlay.html?preview=0.08");

  await expect.poll(async () => (
    page.locator("#blackhole-canvas").evaluate((canvas) => ({
      dpr: canvas.width / canvas.clientWidth,
      raySteps: Number(canvas.dataset.raySteps),
      framesPerSecond: Number(canvas.dataset.framesPerSecond),
      tourEnabled: canvas.dataset.tourEnabled,
      tourSize: Number(canvas.dataset.tourSize),
    }))
  )).toEqual({
    dpr: 1.75,
    raySteps: 56,
    framesPerSecond: 15,
    tourEnabled: "true",
    tourSize: 6,
  });
});
