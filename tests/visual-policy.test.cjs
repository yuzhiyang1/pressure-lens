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

test("默认视觉保持原版流光，不叠加桌面折射蒙层", () => {
  assert.equal(visualPolicy.desktopRefractionEnabled, false);
});

test("减少动画只降低速度，不把黑洞冻结", () => {
  assert.equal(visualPolicy.motionScale(false), 1);
  assert.ok(visualPolicy.motionScale(true) > 0);
  assert.ok(visualPolicy.motionScale(true) < 1);
});
