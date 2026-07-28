#version 300 es
precision highp float;

// WebGL2 透明覆盖层改编自 s0xDk/ghostty-blackhole 的 blackhole.glsl。
// 原作 Copyright (c) 2026 s13k <s13k@pm.me>，采用 MIT License。
// 许可与归属见 THIRD_PARTY_NOTICES.md。测地线加速度、蛙跳积分、薄吸积盘
// 交叉和相对论颜色模型沿用原作思路；透明合成与压力参数为 Pressure Lens 改动。

uniform vec2 uResolution;
uniform float uTime;
uniform float uPressure;
uniform float uPresentationScale;
uniform int uShapeFrom;
uniform int uShapeTo;
uniform float uShapeBlend;
uniform float uRotationPhase;
uniform float uLensStrength;
uniform int uRaySteps;
uniform sampler2D uBackdrop;
uniform float uBackdropReady;
out vec4 fragColor;

const float PI = 3.14159265359;
const float B_CRIT = 2.598076211;
const int N_STEPS = 56;

float hash21(vec2 p) {
    p = fract(p * vec2(123.34, 456.21));
    p += dot(p, p + 45.32);
    return fract(p.x * p.y);
}

float noiseWrapY(vec2 p, float period) {
    vec2 cell = floor(p);
    vec2 blend = fract(p);
    blend = blend * blend * (3.0 - 2.0 * blend);
    float y0 = mod(cell.y, period);
    float y1 = mod(cell.y + 1.0, period);
    return mix(
        mix(
            hash21(vec2(cell.x, y0)),
            hash21(vec2(cell.x + 1.0, y0)),
            blend.x
        ),
        mix(
            hash21(vec2(cell.x, y1)),
            hash21(vec2(cell.x + 1.0, y1)),
            blend.x
        ),
        blend.y
    );
}

float diskTexture(
    float radius,
    float turns,
    float time,
    float speed,
    float wind,
    float contrast,
    float projectedPixelFootprint
) {
    float spiral = radius * wind - time * speed / max(pow(radius, 1.5), 1.0);
    // 角向格点按完整圈数包裹，避免正视形态在 atan 分支处出现水平接缝。
    // 与 Ghostty Blackhole 保持同一组宽纹理频率；过高的 43 圈细层在侧视旋转时
    // 会压进单个屏幕像素，产生点阵和摩尔纹。
    float coarse = noiseWrapY(
        vec2(radius * 1.0, turns * 9.0 + spiral * 1.5 + 7.0),
        9.0
    );
    float detail = noiseWrapY(
        vec2(radius * 2.8, turns * 19.0 + spiral * 3.0),
        19.0
    );
    // 侧视盘会把几十条角向细纹压进一个屏幕像素。继续提高 DPR 只能延后摩尔纹，
    // 这里按投影后的像素足迹逐级收掉不可分辨的细层，保留可见的宽流光。
    float coarseRate = projectedPixelFootprint * (1.0 + 1.5 * abs(wind));
    float detailRate = projectedPixelFootprint * (2.8 + 3.0 * abs(wind));
    float coarseVisibility = 1.0 - smoothstep(0.48, 1.10, coarseRate);
    float detailVisibility = 1.0 - smoothstep(0.24, 0.72, detailRate);
    float filteredCoarse = mix(0.5, coarse, coarseVisibility);
    float filteredDetail = mix(filteredCoarse, detail, detailVisibility);
    float threads = pow(
        clamp(filteredCoarse * 0.35 + filteredDetail * 0.65, 0.0, 1.0),
        max(contrast, 0.35)
    );
    return 0.24 + 1.55 * threads;
}

vec3 thermalColor(float heat, float temperature) {
    vec3 ember = vec3(1.0, 0.075, 0.012);
    vec3 gold = vec3(1.0, 0.43, 0.085);
    vec3 whiteHot = vec3(1.0, 0.91, 0.72);
    vec3 color = mix(ember, gold, smoothstep(0.08, 0.54, heat));
    color = mix(color, whiteHot, smoothstep(0.52, 1.0, heat));
    vec3 quasar = mix(vec3(0.68, 0.82, 1.0), vec3(0.96, 0.98, 1.0), heat);
    return mix(color, quasar, clamp(temperature, 0.0, 1.0) * 0.62);
}

mat2 rotate2(float angle) {
    float cosine = cos(angle);
    float sine = sin(angle);
    return mat2(cosine, -sine, sine, cosine);
}

// 形态参数参考 Ghostty Blackhole Demo Tour，并针对透明桌面减少不可见预设。
struct DiskLook {
    float inclination;
    float roll;
    float inner;
    float outer;
    float gain;
    float opacity;
    float contrast;
    float wind;
    float speed;
    float temperature;
};

DiskLook lookAt(int index) {
    if (index == 1) {
        // Gargantua：接近侧视，盘面收紧，轮廓更克制。
        return DiskLook(1.54, 0.04, 2.15, 6.8, 1.45, 0.52, 1.15, 0.70, 4.2, 0.16);
    }
    if (index == 2) {
        // M87* donut：大幅降低倾角，形成接近环状的吸积盘。
        return DiskLook(0.58, -0.28, 2.25, 6.2, 1.38, 0.42, 0.95, 0.40, 2.7, 0.06);
    }
    if (index == 3) {
        // Face-on ember：近乎正视，外盘展开成宽阔圆环。
        return DiskLook(0.26, 0.02, 2.75, 9.2, 1.16, 0.34, 1.75, 0.82, 4.5, 0.34);
    }
    if (index == 4) {
        // Quasar：更热、更宽、更偏斜，压力高时具有喷薄感。
        return DiskLook(1.12, 0.50, 2.80, 10.5, 1.28, 0.30, 2.35, 1.05, 6.6, 0.92);
    }
    if (index == 5) {
        // Blazar：参考官方 Demo Tour 的高温、宽盘与更强束流感。
        return DiskLook(1.05, 0.55, 3.00, 13.8, 1.05, 0.28, 2.55, 1.18, 7.2, 1.00);
    }
    if (index == 6) {
        // Pure lens：暂时隐去吸积盘，让桌面引力透镜本身成为主角。
        return DiskLook(1.50, 0.35, 1.80, 8.0, 0.00, 0.00, 1.60, 0.72, 5.0, 0.28);
    }
    // Inferno：默认形态，也是循环首尾的稳定锚点。
    return DiskLook(1.48, 0.25, 1.78, 7.8, 1.72, 0.56, 2.10, 0.72, 5.0, 0.28);
}

DiskLook mixLook(DiskLook from, DiskLook to, float amount) {
    return DiskLook(
        mix(from.inclination, to.inclination, amount),
        mix(from.roll, to.roll, amount),
        mix(from.inner, to.inner, amount),
        mix(from.outer, to.outer, amount),
        mix(from.gain, to.gain, amount),
        mix(from.opacity, to.opacity, amount),
        mix(from.contrast, to.contrast, amount),
        mix(from.wind, to.wind, amount),
        mix(from.speed, to.speed, amount),
        mix(from.temperature, to.temperature, amount)
    );
}

vec2 lissajous(float time) {
    return vec2(
        0.75 * sin(time * 0.37) + 0.25 * sin(time * 0.83 + 1.0),
        0.70 * sin(time * 0.54 + 2.1) + 0.30 * sin(time * 1.07)
    );
}

void main() {
    vec2 screen = (gl_FragCoord.xy - 0.5 * uResolution) / uResolution.y;
    float pressure = clamp(uPressure, 0.0, 1.0);
    float eased = pow(pressure, 0.72);
    // 形态由稳定的压力语义驱动；状态变化时只做一次平滑过渡。
    DiskLook look = mixLook(
        lookAt(uShapeFrom),
        lookAt(uShapeTo),
        smoothstep(0.0, 1.0, uShapeBlend)
    );

    // 低压力也保留可辨认的小范围漂移；压力只扩大活动范围，不把黑洞冻结。
    vec2 wander = mix(
        lissajous(uTime * 0.55),
        lissajous(uTime * 1.15),
        eased
    );
    vec2 drift = wander * mix(0.009, 0.019, eased);
    drift += vec2(cos(uTime * 0.80), sin(uTime * 1.00))
        * mix(0.006, 0.010, eased);
    float roll = look.roll
        + uRotationPhase
        + 0.04 * sin(uTime * 0.047)
        + 0.015 * sin(uTime * 0.13 + 1.3);
    vec2 position = rotate2(roll) * (screen - drift);
    // 高压以盘面温度和宽度表达，不再把视觉面积放大到接近窗口边缘。
    // 小型桌面悬浮窗需要更大的低压展示尺度，否则同一组流光会被压进过少像素。
    // 高压形态本身已经更宽，逐步收回放大倍率以保留透明安全区。
    float presentationScale = mix(
        uPresentationScale,
        min(uPresentationScale, 1.18),
        eased
    );
    float shadowRadius = mix(0.082, 0.130, eased) * presentationScale;
    float worldScale = B_CRIT / shadowRadius;
    vec2 rayPlane = position * worldScale;
    float impact = length(rayPlane);

    float diskInner = look.inner;
    float diskOuter = look.outer;
    float traceLimit = diskOuter + 2.8;
    if (impact > traceLimit) {
        discard;
    }

    float cameraZ = 14.0;
    vec3 point = vec3(rayPlane, cameraZ);
    vec3 velocity = vec3(0.0, 0.0, -1.0);
    float angularMomentum2 = dot(rayPlane, rayPlane);
    float inclination = look.inclination;
    float cosineInclination = cos(inclination);
    float sineInclination = sin(inclination);
    float projectedPixelFootprint =
        (worldScale / uResolution.y)
        / max(abs(cosineInclination), 0.32);
    vec3 diskNormal = vec3(0.0, sineInclination, cosineInclination);
    vec3 diskAxisY = vec3(0.0, cosineInclination, -sineInclination);
    vec3 emitted = vec3(0.0);
    float opacity = 0.0;
    float transmittance = 1.0;
    bool captured = false;
    float previousSide = dot(point, diskNormal);
    vec3 previousPoint = point;

    // Schwarzschild 光子测地线的 kick-drift-kick 蛙跳积分。
    for (int index = 0; index < N_STEPS; index++) {
        if (index >= uRaySteps) {
            break;
        }
        float radius2 = dot(point, point);
        if (radius2 < 1.0) {
            captured = true;
            break;
        }
        if (point.z < -cameraZ && velocity.z < 0.0) {
            break;
        }
        if (radius2 > 4.0 * cameraZ * cameraZ) {
            break;
        }

        float radius = sqrt(radius2);
        float stepSize = clamp(0.16 * radius, 0.03, 1.45);
        vec3 acceleration =
            -1.5 * angularMomentum2 * point / (radius2 * radius2 * radius);
        velocity += acceleration * (0.5 * stepSize);
        point += velocity * stepSize;

        radius2 = dot(point, point);
        radius = sqrt(radius2);
        acceleration =
            -1.5 * angularMomentum2 * point / (radius2 * radius2 * radius);
        velocity += acceleration * (0.5 * stepSize);

        float side = dot(point, diskNormal);
        if (side * previousSide < 0.0 && transmittance > 0.025) {
            float crossing = previousSide / (previousSide - side);
            vec3 diskPoint = mix(previousPoint, point, crossing);
            float diskRadius = length(diskPoint);

            if (diskRadius > diskInner && diskRadius < diskOuter) {
                float band =
                    smoothstep(diskInner, diskInner * 1.22, diskRadius)
                    * (1.0 - smoothstep(diskOuter * 0.72, diskOuter, diskRadius));
                float phi = atan(dot(diskPoint, diskAxisY), diskPoint.x);
                float turns = phi / (2.0 * PI);
                float localTime = sqrt(max(1.0 - 1.5 / diskRadius, 0.02));
                float streak = diskTexture(
                    diskRadius,
                    turns,
                    uTime * localTime * mix(0.42, 0.18, eased),
                    look.speed,
                    look.wind,
                    look.contrast,
                    projectedPixelFootprint
                );
                float thermal = pow(diskInner / diskRadius, 0.58);
                vec3 color = thermalColor(thermal, look.temperature);

                // 轨道切向速度制造接近侧蓝移、远离侧红移和不对称增亮。
                vec3 tangent = normalize(cross(diskNormal, diskPoint));
                float beta = min(sqrt(0.5 / diskRadius), 0.54);
                float lineOfSight = dot(tangent, normalize(-velocity));
                float shift = sqrt(max(1.0 - 1.5 / diskRadius, 0.02))
                    / max(1.0 - beta * lineOfSight, 0.28);
                shift = clamp(shift, 0.48, 1.62);
                float approaching = smoothstep(1.02, 1.48, shift);
                float receding = 1.0 - smoothstep(0.58, 0.96, shift);
                color = mix(color, vec3(0.68, 0.84, 1.0), approaching * 0.48);
                color = mix(color, vec3(0.82, 0.025, 0.008), receding * 0.44);

                float emission = band * streak * pow(shift, 2.45) * look.gain;
                emitted += transmittance * color * emission;
                float crossingOpacity = clamp(
                    band * (0.20 + 0.34 * streak) * (look.opacity / 0.56),
                    0.0,
                    0.78
                );
                opacity += transmittance * crossingOpacity;
                transmittance *= 1.0 - crossingOpacity;
            }
        }

        previousSide = side;
        previousPoint = point;
    }

    // 透明覆盖层不能依赖“某个像素刚好命中临界半径”。接近临界半径的光线会
    // 绕行多圈，低能量盘面采样因此容易变成一圈离散棕点；只连续化这部分暗纹，
    // 高亮盘面仍保留测地线积分产生的真实弧线。
    float photonPixelWidth = max(fwidth(impact), 0.008);
    float photonBand = (
        1.0 - smoothstep(
            0.0,
            photonPixelWidth * 3.25,
            abs(impact - B_CRIT)
        )
    );
    float emissionPeak = max(max(emitted.r, emitted.g), emitted.b);
    float lowEnergyRing = photonBand
        * (1.0 - smoothstep(0.65, 2.20, emissionPeak));
    // Ghostty 本身没有额外绘制一圈光环。这里同样不“补亮”临界环，而是把积分
    // 预算造成的暗色离散命中压回事件视界；真正明亮的盘面穿越不会被削弱。
    emitted *= 1.0 - lowEnergyRing;
    opacity *= 1.0 - lowEnergyRing * 0.86;

    if (!captured && opacity < 0.18) {
        vec2 starCell = floor(normalize(velocity).xy * 155.0);
        float star = smoothstep(0.992, 1.0, hash21(starCell));
        float lensWindow = exp(-impact * impact * 0.055);
        float starVisibility = mix(1.0, 0.22, uBackdropReady);
        emitted += vec3(0.54, 0.66, 0.9)
            * star * lensWindow * 0.36 * starVisibility;
        opacity += star * lensWindow * 0.18 * starVisibility;
    }

    vec3 color = vec3(1.0) - exp(-emitted * mix(1.24, 1.48, eased));
    float luminance = dot(color, vec3(0.2126, 0.7152, 0.0722));
    float alpha = clamp(max(opacity, luminance * 1.12), 0.0, 1.0);
    // Ghostty 输出整张不透明终端画面；透明 Tauri 窗口需要显式计算事件视界
    // 的边缘覆盖率，否则 captured 的布尔跳变会直接暴露成锯齿圆周。
    float horizonPixelWidth = max(fwidth(impact) * 1.35, 0.010);
    float horizonCoverage = 1.0 - smoothstep(
        B_CRIT - horizonPixelWidth,
        B_CRIT + horizonPixelWidth,
        impact
    );
    alpha = max(alpha, horizonCoverage * 0.985);

    // 采样悬浮窗下方的真实桌面，并按黑洞中心径向偏移纹理坐标。
    // 屏幕帧只作为这一帧的 GPU 纹理使用，不影响压力计算。
    vec2 backdropUv = vec2(
        gl_FragCoord.x / uResolution.x,
        1.0 - gl_FragCoord.y / uResolution.y
    );
    vec2 lensPosition = screen - drift;
    vec2 radialDirection = length(lensPosition) > 0.0001
        ? normalize(lensPosition)
        : vec2(0.0);
    // 折射只覆盖事件视界附近的局部圆域，避免 420px 透明窗口的物理边界露出来。
    float lensRadius = length(lensPosition);
    float localLensOuter = min(
        max(shadowRadius * 2.80, 0.30),
        0.38
    );
    float localLensWindow = 1.0 - smoothstep(
        shadowRadius * 0.86,
        localLensOuter,
        lensRadius
    );
    float bendProfile = exp(-abs(impact - B_CRIT) * 0.46);
    float bendStrength = localLensWindow
        * bendProfile
        * mix(0.030, 0.058, eased)
        * clamp(uLensStrength, 0.0, 1.0);
    vec2 backdropOffset = vec2(
        radialDirection.x * uResolution.y / uResolution.x,
        -radialDirection.y
    ) * bendStrength;
    vec3 backdrop = texture(
        uBackdrop,
        clamp(backdropUv + backdropOffset, vec2(0.002), vec2(0.998))
    ).rgb;
    float backdropLuminance = dot(
        backdrop,
        vec3(0.2126, 0.7152, 0.0722)
    );
    float lightBackdrop = uBackdropReady
        * smoothstep(0.58, 0.88, backdropLuminance);
    // 背景只影响折射底图，不能改写黑洞前景材质；否则同一压力会在白底变成灰棕色。
    // 暗场只托住事件视界和实际发光像素，不再形成一整块可见的灰色圆形蒙层。
    float horizonSupport = 1.0 - smoothstep(
        shadowRadius * 0.86,
        shadowRadius * 1.45,
        lensRadius
    );
    float materialSupport = max(
        horizonSupport,
        smoothstep(0.008, 0.18, alpha)
    );
    vec3 groundedBackdrop = backdrop * mix(
        1.0,
        0.02,
        lightBackdrop * localLensWindow * materialSupport * 0.92
    );

    // WebView 本身仍是 420×420 的透明窗口。让所有图层在四条物理边界前羽化，
    // 避免宽盘或高压力折射触及窗口边缘时露出一条笔直的截图接缝。
    vec2 edgeUv = gl_FragCoord.xy / uResolution;
    float edgeDistance = min(
        min(edgeUv.x, 1.0 - edgeUv.x),
        min(edgeUv.y, 1.0 - edgeUv.y)
    );
    float edgeFeather = smoothstep(0.012, 0.105, edgeDistance);
    float foregroundAlpha = clamp(alpha * edgeFeather, 0.0, 1.0);

    // 将折射桌面作为黑洞与吸积盘背后的图层；径向与四边同时渐隐。
    float backdropAlpha = uBackdropReady
        * localLensWindow
        * edgeFeather
        * 0.985;
    float finalAlpha = foregroundAlpha + backdropAlpha * (1.0 - foregroundAlpha);
    if (finalAlpha < 0.006) {
        discard;
    }
    vec3 finalColor = (
        color * foregroundAlpha
        + groundedBackdrop * backdropAlpha * (1.0 - foregroundAlpha)
    ) / max(finalAlpha, 0.0001);

    fragColor = vec4(finalColor, finalAlpha);
}
