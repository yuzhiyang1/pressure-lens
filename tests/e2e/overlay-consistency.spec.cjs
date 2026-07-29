const { test, expect } = require("@playwright/test");

// 生产悬浮窗仍是 420×420；门禁缩小 CSS 画布，避免 Windows Runner 的
// SwiftShader 在三次像素采样时超时，同时保留相同 DPR、Shader 和压力参数。
test.use({ viewport: { width: 300, height: 300 } });

async function readBlackHoleCenter(page) {
  const canvasPng = await page.locator("#blackhole-canvas").screenshot();
  return page.evaluate(async (pngBase64) => {
    const response = await fetch(`data:image/png;base64,${pngBase64}`);
    const bitmap = await createImageBitmap(await response.blob());
    const surface = new OffscreenCanvas(bitmap.width, bitmap.height);
    const context = surface.getContext("2d");
    context.drawImage(bitmap, 0, 0);
    bitmap.close();
    const rgba = context.getImageData(0, 0, surface.width, surface.height).data;
    let weightedX = 0;
    let weightedY = 0;
    let totalWeight = 0;

    for (let offset = 0; offset < rgba.length; offset += 4) {
      const red = rgba[offset];
      const green = rgba[offset + 1];
      const blue = rgba[offset + 2];
      const alpha = rgba[offset + 3];
      // 事件视界是唯一接近不透明的黑色区域，用它测中心可排除流光纹理变化。
      if (alpha >= 235 && red <= 16 && green <= 16 && blue <= 16) {
        const pixel = offset / 4;
        const weight = alpha / 255;
        weightedX += (pixel % surface.width) * weight;
        weightedY += Math.floor(pixel / surface.width) * weight;
        totalWeight += weight;
      }
    }

    return {
      x: weightedX / Math.max(totalWeight, 1),
      y: weightedY / Math.max(totalWeight, 1),
    };
  }, canvasPng.toString("base64"));
}

async function readGoldMetrics(page, url) {
  await page.goto(url);
  await expect(page.locator("#desktop-lens")).toHaveClass(/is-ready/);
  await page.locator(".status").evaluate((element) => {
    element.style.display = "none";
  });
  if (url.includes("backdrop=")) {
    await expect.poll(async () => page.evaluate(() => (
      window.PressureOverlayPreview.controller.getDiagnostics().backdropVisibility
    )), { timeout: 20_000 }).toBeGreaterThan(0.9);
  }

  const canvasPng = await page.locator("#blackhole-canvas").screenshot();
  return page.evaluate(async (pngBase64) => {
    const response = await fetch(`data:image/png;base64,${pngBase64}`);
    const bitmap = await createImageBitmap(await response.blob());
    const surface = new OffscreenCanvas(bitmap.width, bitmap.height);
    const context = surface.getContext("2d");
    context.drawImage(bitmap, 0, 0);
    bitmap.close();
    const rgba = context.getImageData(0, 0, surface.width, surface.height).data;
    const gold = [];
    for (let offset = 0; offset < rgba.length; offset += 4) {
      const red = rgba[offset];
      const green = rgba[offset + 1];
      const blue = rgba[offset + 2];
      const alpha = rgba[offset + 3];
      if (
        alpha >= 40
        && red >= green * 1.03
        && green >= blue * 1.12
        && red - blue >= 24
      ) {
        gold.push({
          brightness: Math.max(red, green, blue),
          red,
          green,
          blue,
        });
      }
    }

    gold.sort((left, right) => left.brightness - right.brightness);
    const start = Math.floor(gold.length * 0.75);
    const highlights = gold.slice(start);
    const mean = (channel) => highlights.reduce(
      (total, pixel) => total + pixel[channel],
      0,
    ) / Math.max(highlights.length, 1);

    return {
      count: gold.length,
      red: mean("red"),
      green: mean("green"),
      blue: mean("blue"),
      brightness: mean("brightness"),
    };
  }, canvasPng.toString("base64"));
}

test("悬浮黑洞同时保持材质一致、持续旋转与可见漂移", async ({ page }) => {
  test.setTimeout(90_000);
  const dark = await readGoldMetrics(
    page,
    "/overlay.html?preview=0.13&shape=0&time=2&rotation=0",
  );
  const light = await readGoldMetrics(
    page,
    "/overlay.html?backdrop=0.13&shape=0&time=2&rotation=0",
  );

  expect(light.count).toBeGreaterThan(500);
  expect(light.brightness / dark.brightness).toBeGreaterThanOrEqual(0.98);
  expect(light.brightness / dark.brightness).toBeLessThanOrEqual(1.12);
  expect(light.red / dark.red).toBeGreaterThanOrEqual(0.98);
  expect(light.red / dark.red).toBeLessThanOrEqual(1.12);
  await page.goto("/overlay.html?preview=0.13");
  await expect(page.locator("#desktop-lens")).toHaveClass(/is-ready/);
  const before = await page.evaluate(() => (
    window.PressureOverlayPreview.controller.getDiagnostics()
  ));
  const centerBefore = await readBlackHoleCenter(page);

  await page.waitForTimeout(2_500);
  const after = await page.evaluate(() => (
    window.PressureOverlayPreview.controller.getDiagnostics()
  ));
  const centerAfter = await readBlackHoleCenter(page);
  const centerDistance = Math.hypot(
    centerAfter.x - centerBefore.x,
    centerAfter.y - centerBefore.y,
  );

  expect(after.animationPhase - before.animationPhase).toBeGreaterThanOrEqual(0.55);
  expect(after.rotationPhase - before.rotationPhase).toBeGreaterThanOrEqual(0.055);
  expect(centerDistance).toBeGreaterThanOrEqual(0.5);
});
