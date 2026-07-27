const test = require("node:test");
const assert = require("node:assert/strict");
const { BackdropCaptureGate } = require("../ui/backdrop-capture-gate.js");

test("拖动开始前发出的桌面截图在返回后必须被丢弃", () => {
  const gate = new BackdropCaptureGate();
  const staleGeneration = gate.beginCapture();

  gate.suspend();
  assert.equal(gate.acceptCapture(staleGeneration), false);

  gate.resume();
  const freshGeneration = gate.beginCapture();
  assert.equal(gate.acceptCapture(freshGeneration), true);
});
