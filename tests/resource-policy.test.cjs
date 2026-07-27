const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");

function loadPolicy() {
  const source = fs.readFileSync(
    path.join(__dirname, "..", "ui", "resource-policy.js"),
    "utf8",
  );
  const context = { window: {} };
  vm.runInNewContext(source, context);
  return context.window.PressureResources;
}

test("平衡档遵守常驻渲染预算", () => {
  const policy = loadPolicy().resolve("balanced", {
    animationIntensity: 0.75,
    lensIntensity: 0.65,
  });

  assert.ok(policy.framesPerSecond <= 20);
  assert.ok(policy.maximumDpr <= 1.5);
  assert.ok(policy.raySteps <= 40);
  assert.ok(policy.backdropFramesPerSecond <= 4);
});

test("暂停或隐藏时不允许继续桌面捕获", () => {
  const resources = loadPolicy();
  const base = resources.resolve("balanced");

  assert.equal(resources.captureEnabled(base, { visible: false, paused: false }), false);
  assert.equal(resources.captureEnabled(base, { visible: true, paused: true }), false);
  assert.equal(resources.captureEnabled(base, { visible: true, paused: false }), true);
});
