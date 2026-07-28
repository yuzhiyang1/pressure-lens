(() => {
  const clamp = (value, minimum, maximum) =>
    Math.max(minimum, Math.min(maximum, value));

  async function start(canvas, readPressure, options = {}) {
    if (!window.PressureResources) {
      throw new Error("资源策略未加载");
    }
    if (!window.PressureVisuals) {
      throw new Error("视觉策略未加载");
    }
    let resourcePolicy = window.PressureResources.resolve(
      options.resourceMode ?? "balanced",
      {
        animationIntensity: options.animationIntensity,
        lensIntensity: options.lensIntensity,
        decorativeShapeTour: options.decorativeShapeTour,
      },
    );
    const lifecycle = {
      visible: options.visible !== false,
      paused: false,
    };

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
    const fragmentSource = await fetch("./overlay-shader.frag?v=9").then((response) => {
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
    const shapeFromUniform = gl.getUniformLocation(program, "uShapeFrom");
    const shapeToUniform = gl.getUniformLocation(program, "uShapeTo");
    const shapeBlendUniform = gl.getUniformLocation(program, "uShapeBlend");
    const rotationPhaseUniform = gl.getUniformLocation(program, "uRotationPhase");
    const lensStrengthUniform = gl.getUniformLocation(program, "uLensStrength");
    const rayStepsUniform = gl.getUniformLocation(program, "uRaySteps");
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

    const reduceMotion = matchMedia("(prefers-reduced-motion: reduce)").matches;
    let renderedPressure = clamp(Number(readPressure()) || 0, 0, 1);
    let previousFrameAt = 0;
    let semanticState = String(options.readVisualState?.() ?? "uncertain");
    let shapeFrom = window.PressureVisuals.primaryShape(semanticState);
    let shapeTo = shapeFrom;
    let shapeBlend = 1;
    let shapeTransitionElapsed = 2;
    let decorativeSlot = 0;
    let rotationPhase = 0;
    let animationPhase = 0;
    let pendingBackdrop = null;
    let backdropReady = false;
    let backdropVisibility = 0;
    let backdropTextureWidth = 1;
    let backdropTextureHeight = 1;
    let backdropPixelBuffer = null;
    let backdropDiagnosticsPending = true;
    let disposed = false;
    let renderRequest = null;
    const diagnostics = {
      captureRequests: 0,
      textureAllocations: 1,
      textureUpdates: 0,
    };
    const captureGate = typeof options.readBackdrop === "function"
      ? new window.PressureBackdrop.BackdropCaptureGate()
      : null;

    const decodeBackdrop = async (payload) => {
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
      if (bytes[0] === 0xff && bytes[1] === 0xd8) {
        const bitmap = await createImageBitmap(
          new Blob([bytes], { type: "image/jpeg" }),
        );
        return {
          width: bitmap.width,
          height: bitmap.height,
          brightnessMean: null,
          brightnessRange: null,
          bitmap,
          pixels: null,
        };
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

      // 亮度诊断只计算第一张有效帧，后续帧直接进入纹理上传路径。
      let brightnessMean = null;
      let brightnessRange = null;
      if (backdropDiagnosticsPending) {
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
        brightnessMean = Math.round(brightnessTotal / Math.max(brightnessSamples, 1));
        brightnessRange = maximumBrightness - minimumBrightness;
      }

      const pixelLength = width * height * 4;
      if (!backdropPixelBuffer || backdropPixelBuffer.byteLength !== pixelLength) {
        backdropPixelBuffer = new Uint8Array(pixelLength);
      }
      // 复用一块固定上传缓冲，切断 IPC 消息与 WebGL 调用之间的引用链。
      // 这一次拷贝换来的是长期运行时不随桌面帧数增长的 Renderer 私有内存。
      backdropPixelBuffer.set(bytes.subarray(8));

      return {
        width,
        height,
        brightnessMean,
        brightnessRange,
        pixels: backdropPixelBuffer,
      };
    };

    let captureTimer = null;
    let captureInFlight = false;
    let captureUrgent = false;
    const captureInterval = () => 1000 / resourcePolicy.backdropFramesPerSecond;
    const canCapture = () =>
      Boolean(captureGate)
      && !disposed
      && pendingBackdrop === null
      && window.PressureResources.captureEnabled(resourcePolicy, lifecycle)
      && !captureGate.isSuspended();

    const scheduleBackdropCapture = (delay = captureInterval()) => {
      if (
        !canCapture()
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
      diagnostics.captureRequests += 1;
      try {
        const frame = await decodeBackdrop(await options.readBackdrop());
        // 窗口开始或结束拖动都会推进代次；旧坐标帧即使晚到也不能进入 GPU。
        if (captureGate.acceptCapture(generation)) {
          pendingBackdrop = frame;
        } else {
          frame.bitmap?.close();
        }
      } catch (error) {
        if (captureGate.acceptCapture(generation)) {
          options.onBackdropError?.(error);
        }
      } finally {
        captureInFlight = false;
        if (canCapture()) {
          const delay = captureUrgent ? 0 : captureInterval();
          captureUrgent = false;
          scheduleBackdropCapture(delay);
        }
      }
    };

    if (canCapture()) {
      scheduleBackdropCapture(0);
    }

    const clearBackdropTexture = () => {
      pendingBackdrop?.bitmap?.close();
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
      backdropTextureWidth = 1;
      backdropTextureHeight = 1;
      diagnostics.textureAllocations += 1;

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
      if (!captureGate || !window.PressureResources.captureEnabled(resourcePolicy, lifecycle)) {
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

    const requestRender = () => {
      if (disposed || !lifecycle.visible || lifecycle.paused || renderRequest !== null) {
        return;
      }
      renderRequest = requestAnimationFrame(render);
    };

    const render = (now) => {
      renderRequest = null;
      requestRender();
      const frameInterval = 1000 / resourcePolicy.framesPerSecond;
      if (now - previousFrameAt < frameInterval) {
        return;
      }
      const elapsedSeconds = previousFrameAt === 0
        ? frameInterval / 1000
        : Math.min((now - previousFrameAt) / 1000, 0.1);
      previousFrameAt = now;
      // Windows“减少动画”只降低运动幅度，不把黑洞冻结成静态图。
      const motionScale = window.PressureVisuals.motionScale(reduceMotion);
      animationPhase += elapsedSeconds * resourcePolicy.animationIntensity * motionScale;

      // DPR、帧率和光线步数都由统一资源策略限制，避免视觉参数绕过性能预算。
      const dpr = Math.min(window.devicePixelRatio || 1, resourcePolicy.maximumDpr);
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
      // 默认一个视觉族稳定表达一种状态；用户开启巡游后也只在同一语义族内切换。
      const nextSemanticState = String(options.readVisualState?.() ?? "uncertain");
      const family = window.PressureVisuals.familyFor(nextSemanticState);
      const nextDecorativeSlot = resourcePolicy.decorativeShapeTour
        ? Math.floor(animationPhase / 14) % family.length
        : 0;
      const nextShape = options.shapeOverride == null
        ? family[nextDecorativeSlot]
        : Math.floor(options.shapeOverride);
      if (
        nextSemanticState !== semanticState
        || nextShape !== shapeTo
        || nextDecorativeSlot !== decorativeSlot
      ) {
        shapeFrom = shapeBlend >= 0.5 ? shapeTo : shapeFrom;
        shapeTo = nextShape;
        shapeBlend = 0;
        shapeTransitionElapsed = 0;
        semanticState = nextSemanticState;
        decorativeSlot = nextDecorativeSlot;
      }
      if (options.shapeOverride != null) {
        shapeFrom = Math.floor(options.shapeOverride);
        shapeTo = (shapeFrom + 1) % 7;
        shapeBlend = options.shapeOverride - shapeFrom;
      } else if (shapeBlend < 1) {
        shapeTransitionElapsed += elapsedSeconds;
        shapeBlend = reduceMotion ? 1 : Math.min(1, shapeTransitionElapsed / 2);
      }
      if (resourcePolicy.animationIntensity > 0) {
        // 旋转相位独立累计，压力只改变当下速度，不会因 pressure * time 突然跳角度。
        const rotationRate =
          (0.045 + 0.070 * renderedPressure)
          * resourcePolicy.animationIntensity
          * motionScale;
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
        if (
          backdropTextureWidth !== backdropFrame.width
          || backdropTextureHeight !== backdropFrame.height
        ) {
          if (backdropFrame.bitmap) {
            gl.texImage2D(
              gl.TEXTURE_2D,
              0,
              gl.RGBA,
              gl.RGBA,
              gl.UNSIGNED_BYTE,
              backdropFrame.bitmap,
            );
          } else {
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
          }
          backdropTextureWidth = backdropFrame.width;
          backdropTextureHeight = backdropFrame.height;
          diagnostics.textureAllocations += 1;
        } else {
          // 尺寸稳定时只更新已有纹理，避免每帧重新分配 GPU 资源。
          if (backdropFrame.bitmap) {
            gl.texSubImage2D(
              gl.TEXTURE_2D,
              0,
              0,
              0,
              gl.RGBA,
              gl.UNSIGNED_BYTE,
              backdropFrame.bitmap,
            );
          } else {
            gl.texSubImage2D(
              gl.TEXTURE_2D,
              0,
              0,
              0,
              backdropFrame.width,
              backdropFrame.height,
              gl.RGBA,
              gl.UNSIGNED_BYTE,
              backdropFrame.pixels,
            );
          }
          diagnostics.textureUpdates += 1;
        }
        backdropFrame.bitmap?.close();
        pendingBackdrop = null;
        backdropReady = true;
        // 只有纹理已消费后才安排下一帧，避免 IPC 比渲染器更快时形成隐形队列。
        scheduleBackdropCapture(captureInterval());
        if (wasWaitingForFreshFrame) {
          backdropDiagnosticsPending = false;
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
      gl.uniform1f(timeUniform, animationPhase);
      gl.uniform1f(pressureUniform, renderedPressure);
      gl.uniform1i(shapeFromUniform, shapeFrom);
      gl.uniform1i(shapeToUniform, shapeTo);
      gl.uniform1f(shapeBlendUniform, shapeBlend);
      gl.uniform1f(rotationPhaseUniform, rotationPhase);
      gl.uniform1f(lensStrengthUniform, resourcePolicy.lensIntensity);
      gl.uniform1i(rayStepsUniform, resourcePolicy.raySteps);
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

    const setVisible = (visible) => {
      lifecycle.visible = Boolean(visible);
      if (!lifecycle.visible) {
        suspendBackdrop();
        if (renderRequest !== null) {
          cancelAnimationFrame(renderRequest);
          renderRequest = null;
        }
        return;
      }
      resumeBackdrop();
      requestRender();
    };

    const setPaused = (paused) => {
      lifecycle.paused = Boolean(paused);
      setVisible(lifecycle.visible);
    };

    const setPolicy = (mode, intensities = {}) => {
      resourcePolicy = window.PressureResources.resolve(mode, intensities);
      if (window.PressureResources.captureEnabled(resourcePolicy, lifecycle)) {
        resumeBackdrop();
      } else {
        suspendBackdrop();
      }
      requestRender();
    };

    const dispose = () => {
      disposed = true;
      suspendBackdrop();
      if (renderRequest !== null) {
        cancelAnimationFrame(renderRequest);
        renderRequest = null;
      }
      gl.deleteTexture(backdropTexture);
      gl.deleteBuffer(vertices);
      gl.deleteProgram(program);
    };

    requestRender();
    return Object.freeze({
      suspendBackdrop,
      resumeBackdrop,
      setVisible,
      setPaused,
      setPolicy,
      dispose,
      getDiagnostics: () => ({
        ...diagnostics,
        animationPhase,
        rotationPhase,
      }),
    });
  }

  // 仪表盘与桌面覆盖层使用同一渲染入口，保证视觉与压力映射一致。
  window.PressureBlackHole = Object.freeze({ start });
})();
