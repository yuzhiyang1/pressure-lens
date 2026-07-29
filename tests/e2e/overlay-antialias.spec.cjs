const { test, expect } = require("@playwright/test");

test.use({ viewport: { width: 420, height: 420 } });

async function readAliasingMetrics(page, rotation, time = 2) {
  await page.goto(
    `/overlay.html?preview=0.13&shape=0&time=${time}&rotation=${rotation}`,
  );
  await expect(page.locator("#desktop-lens")).toHaveClass(/is-ready/);
  await page.locator(".status").evaluate((element) => {
    element.style.display = "none";
  });
  const canvasPng = await page.locator("#blackhole-canvas").screenshot();

  return page.evaluate(async (pngBase64) => {
    const response = await fetch(`data:image/png;base64,${pngBase64}`);
    const bitmap = await createImageBitmap(await response.blob());
    const surface = new OffscreenCanvas(bitmap.width, bitmap.height);
    const context = surface.getContext("2d");
    context.drawImage(bitmap, 0, 0);
    bitmap.close();
    const { data } = context.getImageData(0, 0, surface.width, surface.height);
    const width = surface.width;
    const height = surface.height;
    const brightness = new Float32Array(width * height);
    let visiblePixels = 0;
    let hardEdgePixels = 0;
    let intermediateEdgePixels = 0;
    let grainPixels = 0;

    for (let pixel = 0; pixel < brightness.length; pixel += 1) {
      const offset = pixel * 4;
      const red = data[offset];
      const green = data[offset + 1];
      const blue = data[offset + 2];
      const alpha = data[offset + 3] / 255;
      // 只分析吸积盘暖色发光像素，排除黑色事件视界和状态胶囊。
      const gold = red >= green * 1.03
        && green >= blue * 1.10
        && red - blue >= 18;
      brightness[pixel] = gold
        ? Math.max(red, green, blue) * alpha
        : 0;
      if (brightness[pixel] >= 24) {
        visiblePixels += 1;
      }
    }

    for (let y = 1; y < height - 1; y += 1) {
      for (let x = 1; x < width - 1; x += 1) {
        const value = brightness[y * width + x];
        if (value < 24) {
          continue;
        }
        let minimumNeighbor = 255;
        let intermediateNeighbors = 0;
        let cardinalTotal = 0;
        for (let offsetY = -1; offsetY <= 1; offsetY += 1) {
          for (let offsetX = -1; offsetX <= 1; offsetX += 1) {
            if (offsetX === 0 && offsetY === 0) {
              continue;
            }
            const neighbor = brightness[
              (y + offsetY) * width + x + offsetX
            ];
            minimumNeighbor = Math.min(minimumNeighbor, neighbor);
            if (neighbor >= 8 && neighbor < 48) {
              intermediateNeighbors += 1;
            }
            if (Math.abs(offsetX) + Math.abs(offsetY) === 1) {
              cardinalTotal += neighbor;
            }
          }
        }
        const cardinalAverage = cardinalTotal / 4;
        // 长时间差速剪切会把宽流光压成逐像素跳变的砂点；宽带纹理的局部
        // 残差应明显低于自身亮度，不应依赖边缘指标侥幸通过。
        if (
          Math.abs(value - cardinalAverage)
            > Math.max(14, value * 0.24)
        ) {
          grainPixels += 1;
        }
        if (minimumNeighbor < 6) {
          if (value >= 72 && intermediateNeighbors === 0) {
            hardEdgePixels += 1;
          } else if (intermediateNeighbors > 0) {
            intermediateEdgePixels += 1;
          }
        }
      }
    }

    return {
      hardEdgeRatio: hardEdgePixels / Math.max(visiblePixels, 1),
      intermediateEdgeRatio:
        intermediateEdgePixels / Math.max(visiblePixels, 1),
      grainRatio: grainPixels / Math.max(visiblePixels, 1),
      visiblePixels,
    };
  }, canvasPng.toString("base64"));
}

test("旋转到任意角度时吸积盘边缘仍保持连续覆盖", async ({ page }) => {
  test.setTimeout(90_000);
  // 30° 是扫描得到的最差角度，75° 是用户最容易观察到点阵的侧转角。
  const rotations = [0, 30, 75, 90];
  const samples = [];
  for (const rotation of rotations) {
    samples.push({
      rotation,
      ...await readAliasingMetrics(page, rotation),
    });
  }

  const maximumHardEdgeRatio = Math.max(
    ...samples.map((sample) => sample.hardEdgeRatio),
  );
  expect(
    maximumHardEdgeRatio,
    `不同旋转角度的锯齿扫描：${JSON.stringify(samples)}`,
  ).toBeLessThanOrEqual(0.035);
  expect(
    Math.max(...samples.map((sample) => sample.grainRatio)),
    `不同旋转角度的颗粒扫描：${JSON.stringify(samples)}`,
  ).toBeLessThanOrEqual(0.08);
});

test("长时间常驻后吸积盘不会退化成像素砂点", async ({ page }) => {
  const times = [2, 3_600, 20_000, 86_400];
  const samples = [];
  for (const time of times) {
    samples.push({
      time,
      ...await readAliasingMetrics(page, 140, time),
    });
  }
  const fresh = samples[0];

  // 长时间运行允许纹理相位变化，但局部颗粒度不能显著高于启动状态。
  expect(
    Math.max(...samples.map((sample) => sample.grainRatio)),
    `不同运行时长的颗粒扫描：${JSON.stringify(samples)}`,
  ).toBeLessThanOrEqual(
    fresh.grainRatio + 0.03,
  );
});
