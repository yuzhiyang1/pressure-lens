const { test, expect } = require("@playwright/test");

// 与 Tauri 主窗口保持同尺寸；首屏断言因此直接代表真实 Windows 布局。
test.use({ viewport: { width: 1180, height: 820 } });

async function installLightweightRenderer(page) {
  // 表单、文案与字号测试不验证 WebGL。使用同控制器接口的轻量桩，
  // 避免 CI 的 SwiftShader 把无关测试耗时放大到 30 秒以上。
  await page.route("**/blackhole-renderer.js*", (route) => route.fulfill({
    contentType: "application/javascript",
    body: `
      window.PressureBlackHole = Object.freeze({
        start: async (canvas) => {
          canvas.width = 1;
          canvas.height = 1;
          return Object.freeze({
            setVisible() {},
            setPaused() {},
            setPolicy() {},
            dispose() {},
          });
        },
      });
    `,
  }));
}

test("高压状态展示可信度、真实历史和恢复动作", async ({ page }) => {
  await page.goto("/index.html?preview=78");

  await expect(page.locator("#score")).toHaveText("78");
  await expect(page.locator("#decision-overview")).toBeVisible();
  await expect(page.locator("#reasons li")).toHaveCount(3);
  await expect(page.locator("#self-report")).toBeVisible();
  await expect(page.locator("#confidence-value")).toHaveText("82%");
  await expect(page.locator("#summary-peak")).toHaveText("76");
  await expect(page.locator("#history-empty")).toBeHidden();
  await expect(page.locator("#advice-title")).toHaveText("离开屏幕五分钟");
  const adviceBox = await page.locator("#advice-panel").boundingBox();
  expect(adviceBox).not.toBeNull();
  expect(adviceBox.y + adviceBox.height).toBeLessThanOrEqual(820);

  // 真实浏览器必须完成 WebGL2 Shader 编译并生成非零帧缓冲。
  await expect.poll(async () => page.locator("#dashboard-blackhole").evaluate((canvas) => ({
    width: canvas.width,
    height: canvas.height,
    ready: canvas.closest("figure")?.classList.contains("is-ready"),
  }))).toMatchObject({ ready: true });
  const canvasSize = await page.locator("#dashboard-blackhole").evaluate((canvas) => ({
    width: canvas.width,
    height: canvas.height,
    dpr: canvas.width / canvas.clientWidth,
    raySteps: Number(canvas.dataset.raySteps),
    framesPerSecond: Number(canvas.dataset.framesPerSecond),
    tourEnabled: canvas.dataset.tourEnabled,
    shapeTo: Number(canvas.dataset.shapeTo),
  }));
  expect(canvasSize.width).toBeGreaterThan(0);
  expect(canvasSize.height).toBeGreaterThan(0);
  expect(canvasSize.dpr).toBeCloseTo(1.5, 2);
  expect(canvasSize).toMatchObject({
    raySteps: 52,
    framesPerSecond: 15,
    tourEnabled: "false",
    shapeTo: 4,
  });
});

test("设置页可暂停采集并保留隐私开关", async ({ page }) => {
  await installLightweightRenderer(page);
  await page.goto("/index.html?preview=55");
  await page.getByRole("button", { name: "设置" }).click();

  const pause = page.getByRole("checkbox", { name: /暂停全部压力采集/ });
  await pause.check();
  await page.getByRole("button", { name: "保存设置" }).click();

  await expect(pause).toBeChecked();
  await expect(page.locator("#settings-status")).toHaveText("预览设置已生效");
  await expect(page.getByRole("checkbox", { name: /键盘节奏/ })).toBeChecked();
  await expect(page.getByRole("checkbox", { name: /Agent Provider/ })).toBeChecked();
  await expect(page.getByRole("checkbox", { name: /演示六种形态/ })).not.toBeChecked();
});

test("设置页可在三种桌面背景实时预览黑洞", async ({ page }) => {
  test.setTimeout(60_000);
  await page.goto("/index.html?preview=55");
  // 先让仪表盘控制器完成初始化，再切页，验证两个渲染器不会争抢首帧。
  await expect(page.locator("#orbit")).toHaveClass(/is-ready/);
  await page.getByRole("button", { name: "设置" }).click();

  await expect(page.getByRole("heading", { name: "实时预览桌面效果" })).toBeVisible();
  const stage = page.locator("#settings-preview-stage");
  await expect(stage).toHaveAttribute("data-preview-background", "dark");
  await expect.poll(async () => page.locator("#settings-blackhole-preview").evaluate((canvas) => ({
    ready: canvas.closest("figure")?.classList.contains("is-ready"),
    width: canvas.width,
    height: canvas.height,
  }))).toMatchObject({ ready: true });

  await page.getByRole("button", { name: "浅色桌面" }).click();
  await expect(stage).toHaveAttribute("data-preview-background", "light");
  await expect(page.getByRole("button", { name: "浅色桌面" })).toHaveAttribute("aria-pressed", "true");
  await expect(page.locator("#settings-preview-status")).toContainText("浅色");

  await page.getByRole("button", { name: "复杂桌面" }).click();
  await expect(stage).toHaveAttribute("data-preview-background", "complex");
  await expect(page.getByRole("button", { name: "复杂桌面" })).toHaveAttribute("aria-pressed", "true");
  await expect(page.locator("#settings-preview-status")).toContainText("复杂");
});

test("关键辅助信息保持至少 12 像素的可读字号", async ({ page }) => {
  await installLightweightRenderer(page);
  await page.goto("/index.html?preview=55");
  const dashboardHelpers = page.locator(
    ".metric-grid small, #calibration-detail, .health-list li, .summary-grid span, .agent-source-copy p",
  );
  const dashboardSizes = await dashboardHelpers.evaluateAll((elements) =>
    elements.map((element) => Number.parseFloat(getComputedStyle(element).fontSize)),
  );
  expect(Math.min(...dashboardSizes)).toBeGreaterThanOrEqual(12);

  await page.getByRole("button", { name: "设置" }).click();
  const settingsHelpers = page.locator(
    ".settings-form label small, #settings-status, #update-status, .danger-zone p:not(.eyebrow)",
  );
  const settingsSizes = await settingsHelpers.evaluateAll((elements) =>
    elements.map((element) => Number.parseFloat(getComputedStyle(element).fontSize)),
  );
  expect(Math.min(...settingsSizes)).toBeGreaterThanOrEqual(12);
});

test("低压预览不会沿用高压原因文案", async ({ page }) => {
  await installLightweightRenderer(page);
  await page.goto("/index.html?preview=13");

  await expect(page.locator("#score")).toHaveText("13");
  await expect(page.locator("#reasons")).toContainText("接近今天的个人基线");
  await expect(page.locator("#reasons")).not.toContainText("恢复窗口正在变窄");
});
