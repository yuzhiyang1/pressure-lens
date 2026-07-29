const test = require("node:test");
const assert = require("node:assert/strict");
const visualPolicy = require("../ui/visual-policy.js");

test("启动和低压力都以第一版 Inferno 倾斜形态为视觉锚点", () => {
  assert.equal(visualPolicy.primaryShape("uncertain"), 0);
  assert.equal(visualPolicy.primaryShape("calm"), 0);
  assert.deepEqual(visualPolicy.familyFor("calm"), [0, 2]);
});

test("高压力仍保留更热、更宽的形态族", () => {
  assert.deepEqual(visualPolicy.familyFor("overloaded"), [4, 5, 0]);
});

test("装饰巡游在任何压力语义下都会经过六种可见形态", () => {
  assert.deepEqual(visualPolicy.tourFor("uncertain"), [0, 1, 2, 3, 4, 5]);
  assert.deepEqual(visualPolicy.tourFor("focused"), [1, 0, 2, 3, 4, 5]);
  assert.equal(visualPolicy.tourSlot(9.99, 6), 0);
  assert.equal(visualPolicy.tourSlot(10, 6), 1);
  assert.equal(visualPolicy.tourSlot(60, 6), 0);
});

test("静止悬浮时默认启用桌面折射", () => {
  assert.equal(visualPolicy.desktopRefractionEnabled, true);
});

test("减少动画只降低速度，不把黑洞冻结", () => {
  assert.equal(visualPolicy.motionScale(false), 1);
  assert.ok(visualPolicy.motionScale(true) > 0);
  assert.ok(visualPolicy.motionScale(true) < 1);
});

test("0 到 25 分共享低压视觉死区，超过死区后才连续增强", () => {
  assert.equal(visualPolicy.visualPressure(0), 0);
  assert.equal(visualPolicy.visualPressure(0.13), 0);
  assert.equal(visualPolicy.visualPressure(0.25), 0);
  assert.equal(visualPolicy.visualPressure(0.625), 0.5);
  assert.equal(visualPolicy.visualPressure(1), 1);
});

test("低压黑洞保持轻微流动，但明显慢于高压状态", () => {
  const zeroPressureMotion = visualPolicy.motionScale(false, 0);
  const lowPressureMotion = visualPolicy.motionScale(false, 0.13);
  const highPressureMotion = visualPolicy.motionScale(false, 0.8);

  assert.ok(zeroPressureMotion > 0);
  assert.equal(lowPressureMotion, zeroPressureMotion);
  assert.ok(lowPressureMotion >= 0.4);
  assert.ok(lowPressureMotion < highPressureMotion);
  assert.equal(visualPolicy.motionScale(false, 1), 1);
});

test("压力持续高于 45 分 20 秒后才从平稳进入升高", () => {
  const controller = visualPolicy.createPressureStateController("calm");

  assert.equal(controller.update(0.45, 0), "calm");
  assert.equal(controller.update(0.62, 19_999), "calm");
  assert.equal(controller.update(0.62, 20_000), "focused");
});

test("压力持续高于 75 分 15 秒后才从升高进入过载", () => {
  const controller = visualPolicy.createPressureStateController("focused");

  assert.equal(controller.update(0.75, 1_000), "focused");
  assert.equal(controller.update(0.9, 15_999), "focused");
  assert.equal(controller.update(0.9, 16_000), "overloaded");
});

test("压力持续低于 35 分 45 秒后才从升高恢复平稳", () => {
  const controller = visualPolicy.createPressureStateController("focused");

  assert.equal(controller.update(0.35, 500), "focused");
  assert.equal(controller.update(0.2, 45_499), "focused");
  assert.equal(controller.update(0.2, 45_500), "calm");
});

test("压力持续低于 65 分 60 秒后才从过载回到升高", () => {
  const controller = visualPolicy.createPressureStateController("overloaded");

  assert.equal(controller.update(0.65, 2_000), "overloaded");
  assert.equal(controller.update(0.4, 61_999), "overloaded");
  assert.equal(controller.update(0.4, 62_000), "focused");
});

test("候选方向改变时重新开始持续确认计时", () => {
  const controller = visualPolicy.createPressureStateController("focused");

  assert.equal(controller.update(0.8, 0), "focused");
  assert.equal(controller.update(0.2, 10_000), "focused");
  assert.equal(controller.update(0.2, 54_999), "focused");
  assert.equal(controller.update(0.2, 55_000), "calm");
});
