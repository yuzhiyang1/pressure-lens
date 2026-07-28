const { test, expect } = require("@playwright/test");

test.use({ viewport: { width: 420, height: 420 } });

test("低压悬浮黑洞保持桌面可读尺寸与完整流光", async ({ page }) => {
  await page.goto("/overlay.html?preview=0.04&shape=0&time=2&rotation=0");
  await expect(page.locator("#desktop-lens")).toHaveClass(/is-ready/);
  await page.locator(".status").evaluate((element) => {
    element.style.display = "none";
  });

  const canvasPng = await page.locator("#blackhole-canvas").screenshot();
  const metrics = await page.evaluate(async (pngBase64) => {
    const response = await fetch(`data:image/png;base64,${pngBase64}`);
    const bitmap = await createImageBitmap(await response.blob());
    const surface = new OffscreenCanvas(bitmap.width, bitmap.height);
    const context = surface.getContext("2d");
    context.drawImage(bitmap, 0, 0);
    bitmap.close();
    const rgba = context.getImageData(0, 0, surface.width, surface.height).data;
    let minimumX = surface.width;
    let minimumY = surface.height;
    let maximumX = -1;
    let maximumY = -1;
    let peakBrightness = 0;

    for (let y = 0; y < surface.height; y += 1) {
      for (let x = 0; x < surface.width; x += 1) {
        const offset = (y * surface.width + x) * 4;
        const brightness = Math.max(
          rgba[offset],
          rgba[offset + 1],
          rgba[offset + 2],
        );
        peakBrightness = Math.max(peakBrightness, brightness);
        if (brightness < 52) {
          continue;
        }
        minimumX = Math.min(minimumX, x);
        minimumY = Math.min(minimumY, y);
        maximumX = Math.max(maximumX, x);
        maximumY = Math.max(maximumY, y);
      }
    }

    return {
      width: maximumX - minimumX + 1,
      height: maximumY - minimumY + 1,
      minimumX,
      minimumY,
      maximumX,
      maximumY,
      peakBrightness,
    };
  }, canvasPng.toString("base64"));

  // 420px 悬浮窗中至少保留约 2/3 宽度给黑洞本体，避免流光挤成棕色小球。
  expect(metrics.width).toBeGreaterThanOrEqual(270);
  expect(metrics.height).toBeGreaterThanOrEqual(180);
  expect(metrics.peakBrightness).toBeGreaterThanOrEqual(220);
  // 仍保留透明安全区，宽形态和局部折射不能露出方形窗口边界。
  expect(metrics.minimumX).toBeGreaterThanOrEqual(28);
  expect(metrics.maximumX).toBeLessThanOrEqual(392);
  expect(metrics.minimumY).toBeGreaterThanOrEqual(28);
  expect(metrics.maximumY).toBeLessThanOrEqual(392);
});
