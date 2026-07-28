const { test, expect } = require("@playwright/test");

test("桌面黑洞使用超采样抑制旋转时的锯齿和摩尔纹", async ({ page }) => {
  await page.goto("/overlay.html?preview=0.08");

  await expect.poll(async () => (
    page.locator("#blackhole-canvas").evaluate((canvas) => (
      canvas.width / canvas.clientWidth
    ))
  )).toBeGreaterThanOrEqual(1.45);
});
