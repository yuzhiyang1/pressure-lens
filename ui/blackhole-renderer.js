(() => {
  const clamp = (value, minimum, maximum) =>
    Math.max(minimum, Math.min(maximum, value));

  async function start(canvas, readPressure, options = {}) {
    const gl = canvas.getContext("webgl2", {
      alpha: true,
      antialias: false,
      depth: false,
      premultipliedAlpha: false,
      preserveDrawingBuffer: false,
    });

    if (!gl) {
      throw new Error("WebGL2 不可用");
    }

    const vertexSource = `#version 300 es
      in vec2 aPosition;
      void main() {
        gl_Position = vec4(aPosition, 0.0, 1.0);
      }
    `;
    const fragmentSource = await fetch("./overlay-shader.frag?v=8").then((response) => {
      if (!response.ok) {
        throw new Error(`Shader 加载失败：${response.status}`);
      }
      return response.text();
    });

    const compile = (type, source) => {
      const shader = gl.createShader(type);
      gl.shaderSource(shader, source);
      gl.compileShader(shader);
      if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
        throw new Error(gl.getShaderInfoLog(shader) || "Shader 编译失败");
      }
      return shader;
    };

    const program = gl.createProgram();
    gl.attachShader(program, compile(gl.VERTEX_SHADER, vertexSource));
    gl.attachShader(program, compile(gl.FRAGMENT_SHADER, fragmentSource));
    gl.linkProgram(program);
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
      throw new Error(gl.getProgramInfoLog(program) || "Shader 链接失败");
    }

    const vertices = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, vertices);
    gl.bufferData(
      gl.ARRAY_BUFFER,
      new Float32Array([-1, -1, 1, -1, -1, 1, -1, 1, 1, -1, 1, 1]),
      gl.STATIC_DRAW,
    );
    gl.useProgram(program);
    const position = gl.getAttribLocation(program, "aPosition");
    gl.enableVertexAttribArray(position);
    gl.vertexAttribPointer(position, 2, gl.FLOAT, false, 0, 0);

    const resolutionUniform = gl.getUniformLocation(program, "uResolution");
    const timeUniform = gl.getUniformLocation(program, "uTime");
    const pressureUniform = gl.getUniformLocation(program, "uPressure");
    const shapePhaseUniform = gl.getUniformLocation(program, "uShapePhase");
    const rotationPhaseUniform = gl.getUniformLocation(program, "uRotationPhase");
    const backdropUniform = gl.getUniformLocation(program, "uBackdrop");
    const backdropReadyUniform = gl.getUniformLocation(program, "uBackdropReady");
    const backdropTexture = gl.createTexture();
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, backdropTexture);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    gl.texImage2D(
      gl.TEXTURE_2D,
      0,
      gl.RGBA,
      1,
      1,
      0,
      gl.RGBA,
      gl.UNSIGNED_BYTE,
      new Uint8Array([0, 0, 0, 0]),
    );
    gl.uniform1i(backdropUniform, 0);

    const startedAt = performance.now();
    const maximumDpr = options.maximumDpr ?? 1.5;
    const supersample = options.supersample ?? 1;
    const frameInterval = 1000 / (options.framesPerSecond ?? 30);
    const reduceMotion = matchMedia("(prefers-reduced-motion: reduce)").matches;
    let renderedPressure = clamp(Number(readPressure()) || 0, 0, 1);
    let previousFrameAt = 0;
    let shapePhase = 0;
    let rotationPhase = 0;
    let pendingBackdrop = null;
    let backdropReady = false;
    let backdropVisibility = 0;
    const captureGate = typeof options.readBackdrop === "function"
      ? new window.PressureBackdrop.BackdropCaptureGate()
      : null;

    const decodeBackdrop = (payload) => {
      let bytes;
      if (payload instanceof ArrayBuffer) {
        bytes = new Uint8Array(payload);
      } else if (ArrayBuffer.isView(payload)) {
        bytes = new Uint8Array(payload.buffer, payload.byteOffset, payload.byteLength);
      } else if (Array.isArray(payload)) {
        bytes = Uint8Array.from(payload);
      } else {
        throw new Error("桌面帧不是可识别的二进制数据");
      }

      if (bytes.byteLength < 8) {
        throw new Error("桌面帧缺少尺寸信息");
      }
      const header = new DataView(bytes.buffer, bytes.byteOffset, 8);
      const width = header.getUint32(0, true);
      const height = header.getUint32(4, true);
      const expectedLength = width * height * 4 + 8;
      if (
        width === 0
        || height === 0
        || !Number.isSafeInteger(expectedLength)
        || bytes.byteLength !== expectedLength
      ) {
        throw new Error("桌面帧尺寸与像素数据不匹配");
      }

      // 仅抽样计算首帧诊断数据，不保留额外图像副本，也不输出任何像素内容。
      let minimumBrightness = 255;
      let maximumBrightness = 0;
      let brightnessTotal = 0;
      let brightnessSamples = 0;
      for (let offset = 8; offset < bytes.byteLength; offset += 64) {
        const brightness = Math.round(
          bytes[offset] * 0.2126
          + bytes[offset + 1] * 0.7152
          + bytes[offset + 2] * 0.0722,
        );
        minimumBrightness = Math.min(minimumBrightness, brightness);
        maximumBrightness = Math.max(maximumBrightness, brightness);
        brightnessTotal += brightness;
        brightnessSamples += 1;
      }

      return {
        width,
        height,
        brightnessMean: Math.round(brightnessTotal / Math.max(brightnessSamples, 1)),
        brightnessRange: maximumBrightness - minimumBrightness,
        // 复制出独立视图，让 Tauri IPC 响应可以立刻释放。
        pixels: bytes.slice(8),
      };
    };

    let captureTimer = null;
    let captureInFlight = false;
    let captureUrgent = false;
    const captureInterval = 1000 / (options.backdropFramesPerSecond ?? 12);

    const scheduleBackdropCapture = (delay = captureInterval) => {
      if (
        !captureGate
        || captureGate.isSuspended()
        || captureInFlight
        || captureTimer !== null
      ) {
        return;
      }
      captureTimer = window.setTimeout(() => {
        captureTimer = null;
        captureNextBackdrop();
      }, delay);
    };

    const captureNextBackdrop = async () => {
      const generation = captureGate?.beginCapture();
      if (generation == null) {
        return;
      }

      captureInFlight = true;
      try {
        const frame = decodeBackdrop(await options.readBackdrop());
        // 窗口开始或结束拖动都会推进代次；旧坐标帧即使晚到也不能进入 GPU。
        if (captureGate.acceptCapture(generation)) {
          pendingBackdrop = frame;
        }
      } catch (error) {
        if (captureGate.acceptCapture(generation)) {
          options.onBackdropError?.(error);
        }
      } finally {
        captureInFlight = false;
        if (!captureGate.isSuspended()) {
          const delay = captureUrgent ? 0 : captureInterval;
          captureUrgent = false;
          scheduleBackdropCapture(delay);
        }
      }
    };

    if (captureGate) {
      scheduleBackdropCapture(0);
    }

    const clearBackdropTexture = () => {
      pendingBackdrop = null;
      backdropReady = false;
      backdropVisibility = 0;
      gl.activeTexture(gl.TEXTURE0);
      gl.bindTexture(gl.TEXTURE_2D, backdropTexture);
      gl.texImage2D(
        gl.TEXTURE_2D,
        0,
        gl.RGBA,
        1,
        1,
        0,
        gl.RGBA,
        gl.UNSIGNED_BYTE,
        new Uint8Array([0, 0, 0, 0]),
      );

      // 原生窗口拖动可能暂时阻塞 WebView 帧循环，因此这里同步重绘纯黑洞帧。
      gl.useProgram(program);
      gl.uniform1f(backdropReadyUniform, 0);
      gl.drawArrays(gl.TRIANGLES, 0, 6);
      gl.flush();
    };

    const suspendBackdrop = () => {
      if (!captureGate) {
        return;
      }
      captureGate.suspend();
      captureUrgent = false;
      if (captureTimer !== null) {
        window.clearTimeout(captureTimer);
        captureTimer = null;
      }
      clearBackdropTexture();
    };

    const resumeBackdrop = () => {
      if (!captureGate) {
        return;
      }
      captureGate.resume();
      clearBackdropTexture();
      captureUrgent = true;
      if (captureTimer !== null) {
        window.clearTimeout(captureTimer);
        captureTimer = null;
      }
      if (!captureInFlight) {
        captureUrgent = false;
        scheduleBackdropCapture(0);
      }
    };

    const render = (now) => {
      requestAnimationFrame(render);
      if (now - previousFrameAt < frameInterval) {
        return;
      }
      const elapsedSeconds = previousFrameAt === 0
        ? frameInterval / 1000
        : Math.min((now - previousFrameAt) / 1000, 0.1);
      previousFrameAt = now;

      // 透明桌面上的高反差边缘更容易暴露锯齿，允许覆盖层主动超采样后再缩小。
      const dpr = Math.min((window.devicePixelRatio || 1) * supersample, maximumDpr);
      const width = Math.max(1, Math.round(canvas.clientWidth * dpr));
      const height = Math.max(1, Math.round(canvas.clientHeight * dpr));
      if (canvas.width !== width || canvas.height !== height) {
        canvas.width = width;
        canvas.height = height;
        gl.viewport(0, 0, width, height);
      }

      // 指数平滑让实时压力变化有重量感，同时避免突然放大造成视觉打扰。
      const targetPressure = clamp(Number(readPressure()) || 0, 0, 1);
      renderedPressure += (targetPressure - renderedPressure) * 0.055;
      // 累计相位避免用 pressure * time 引发长时间运行后的形态瞬移。
      if (!reduceMotion && options.shapeOverride == null) {
        // 七个 Ghostty 风格造型约每 7~12 秒进入下一种，完整巡游约 48~82 秒。
        const shapeRate = 0.085 + (0.145 - 0.085) * renderedPressure;
        shapePhase = (shapePhase + elapsedSeconds * shapeRate) % 7;
      }
      if (!reduceMotion) {
        // 旋转相位独立累计，压力只改变当下速度，不会因 pressure * time 突然跳角度。
        const rotationRate = 0.045 + 0.070 * renderedPressure;
        rotationPhase = (rotationPhase + elapsedSeconds * rotationRate) % (Math.PI * 2);
      }
      gl.clearColor(0, 0, 0, 0);
      gl.clear(gl.COLOR_BUFFER_BIT);
      gl.useProgram(program);
      if (pendingBackdrop) {
        const backdropFrame = pendingBackdrop;
        const wasWaitingForFreshFrame = !backdropReady;
        gl.activeTexture(gl.TEXTURE0);
        gl.bindTexture(gl.TEXTURE_2D, backdropTexture);
        gl.texImage2D(
          gl.TEXTURE_2D,
          0,
          gl.RGBA,
          backdropFrame.width,
          backdropFrame.height,
          0,
          gl.RGBA,
          gl.UNSIGNED_BYTE,
          backdropFrame.pixels,
        );
        pendingBackdrop = null;
        backdropReady = true;
        if (wasWaitingForFreshFrame) {
          options.onBackdropReady?.({
            width: backdropFrame.width,
            height: backdropFrame.height,
            brightnessMean: backdropFrame.brightnessMean,
            brightnessRange: backdropFrame.brightnessRange,
          });
        }
      } else {
        gl.activeTexture(gl.TEXTURE0);
        gl.bindTexture(gl.TEXTURE_2D, backdropTexture);
      }
      gl.uniform2f(resolutionUniform, width, height);
      gl.uniform1f(timeUniform, reduceMotion ? 0 : (now - startedAt) / 1000);
      gl.uniform1f(pressureUniform, renderedPressure);
      gl.uniform1f(shapePhaseUniform, options.shapeOverride ?? shapePhase);
      gl.uniform1f(rotationPhaseUniform, rotationPhase);
      if (!backdropReady || captureGate?.isSuspended()) {
        backdropVisibility = 0;
      } else {
        // 新位置首帧到达后约 120ms 淡入，隐藏纹理恢复时的硬切。
        backdropVisibility = Math.min(
          1,
          backdropVisibility + elapsedSeconds / 0.12,
        );
      }
      gl.uniform1f(backdropReadyUniform, backdropVisibility);
      gl.drawArrays(gl.TRIANGLES, 0, 6);
    };

    requestAnimationFrame(render);
    return Object.freeze({ suspendBackdrop, resumeBackdrop });
  }

  // 仪表盘与桌面覆盖层使用同一渲染入口，保证视觉与压力映射一致。
  window.PressureBlackHole = Object.freeze({ start });
})();
