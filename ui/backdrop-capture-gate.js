((root, factory) => {
  const api = factory();
  if (typeof module === "object" && module.exports) {
    module.exports = api;
  }
  root.PressureBackdrop = api;
})(globalThis, () => {
  class BackdropCaptureGate {
    #generation = 0;
    #suspended = false;

    beginCapture() {
      return this.#suspended ? null : this.#generation;
    }

    acceptCapture(generation) {
      return !this.#suspended && generation === this.#generation;
    }

    suspend() {
      if (!this.#suspended) {
        // 每次进入拖动都推进代次，使已经在途的旧坐标截图永久失效。
        this.#generation += 1;
        this.#suspended = true;
      }
    }

    resume() {
      if (this.#suspended) {
        // 恢复时再次推进代次，只允许新位置发起的截图进入纹理。
        this.#generation += 1;
        this.#suspended = false;
      }
    }

    isSuspended() {
      return this.#suspended;
    }
  }

  return Object.freeze({ BackdropCaptureGate });
});
