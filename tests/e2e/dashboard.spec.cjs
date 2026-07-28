const { test, expect } = require("@playwright/test");

test("高压状态展示可信度、真实历史和恢复动作", async ({ page }) => {
  await page.goto("/index.html?preview=78");

  await expect(page.locator("#score")).toHaveText("78");
  await expect(page.locator("#confidence-value")).toHaveText("82%");
  await expect(page.locator("#summary-peak")).toHaveText("76");
  await expect(page.locator("#history-empty")).toBeHidden();
  await expect(page.locator("#advice-title")).toHaveText("离开屏幕五分钟");

  // 真实浏览器必须完成 WebGL2 Shader 编译并生成非零帧缓冲。
  await expect.poll(async () => page.locator("#dashboard-blackhole").evaluate((canvas) => ({
    width: canvas.width,
    height: canvas.height,
    ready: canvas.closest("figure")?.classList.contains("is-ready"),
  }))).toMatchObject({ ready: true });
  const canvasSize = await page.locator("#dashboard-blackhole").evaluate((canvas) => ({
    width: canvas.width,
    height: canvas.height,
  }));
  expect(canvasSize.width).toBeGreaterThan(0);
  expect(canvasSize.height).toBeGreaterThan(0);
});

test("设置页可暂停采集并保留隐私开关", async ({ page }) => {
  await page.goto("/index.html?preview=55");
  await page.getByRole("button", { name: "设置" }).click();

  const pause = page.getByRole("checkbox", { name: /暂停全部压力采集/ });
  await pause.check();
  await page.getByRole("button", { name: "保存设置" }).click();

  await expect(pause).toBeChecked();
  await expect(page.locator("#settings-status")).toHaveText("预览设置已生效");
  await expect(page.getByRole("checkbox", { name: /键盘节奏/ })).toBeChecked();
  await expect(page.getByRole("checkbox", { name: /Agent Provider/ })).toBeChecked();
});
