/*
 * Liquid glass untuk panel utama UI engram.
 *
 * Shader refraksi diadaptasi dari liquid-glass-js (container.js):
 *   https://github.com/dashersw/liquid-glass-js
 *   MIT License, Copyright (c) 2025 Armagan Amcalar
 *
 *   Permission is hereby granted, free of charge, to any person obtaining a copy
 *   of this software and associated documentation files (the "Software"), to deal
 *   in the Software without restriction, including without limitation the rights
 *   to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 *   copies of the Software, and to permit persons to whom the Software is
 *   furnished to do so, subject to the following conditions:
 *
 *   The above copyright notice and this permission notice shall be included in all
 *   copies or substantial portions of the Software.
 *
 *   THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 *   IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 *   FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 *   AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 *   LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 *   OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 *   SOFTWARE.
 *
 * Perbedaan dari aslinya:
 * - Tanpa html2canvas. Yang dibiaskan adalah wallpaper yang digambar sendiri
 *   (#lg-wallpaper, tetap di viewport), jadi selalu akurat walau isi halaman
 *   berubah dan tidak ada potret halaman yang basi.
 * - Kaca dipasang ke elemen yang sudah ada (sidebar, top bar, drawer, toast,
 *   palet), bukan elemen baru buatan library.
 * - Tengah panel semi-transparan (u_centerAlpha) supaya konten di belakang tetap
 *   terlihat lewat backdrop-filter CSS; tepi tetap membiaskan penuh.
 * - Warna tint mengikuti tema terang/gelap, plus sorotan tipis di tepi atas.
 * - Render hanya saat ada perubahan (resize, transisi, scroll), bukan tiap frame.
 * Tanpa WebGL atau saat pengguna memilih "kurangi transparansi", UI tetap
 * memakai kaca CSS biasa.
 */
(function () {
  "use strict";

  // Panel yang diberi kaca WebGL. Jumlahnya sengaja sedikit: browser membatasi
  // jumlah konteks WebGL aktif (sekitar 16).
  const SELECTORS = ["#sidebar", ".top", "#drawer", "#toast", ".pal-card"];

  const mm = (q) => (window.matchMedia ? window.matchMedia(q) : null);
  const reduce = mm("(prefers-reduced-transparency: reduce)");
  if (reduce && reduce.matches) return;
  if (!document.createElement("canvas").getContext("webgl")) return;

  const root = document.documentElement;
  const surfaces = new Map(); // element -> Surface
  let wallpaper = null;
  let wallpaperVersion = 0;

  // ------------------------------------------------------------ wallpaper
  function isDark() {
    const t = root.dataset.theme;
    if (t === "dark") return true;
    if (t === "light") return false;
    const q = mm("(prefers-color-scheme: dark)");
    return !!(q && q.matches);
  }

  const PALETTES = {
    light: {
      base: ["#f3f6fd", "#e6ecf8", "#eef1fb"],
      blobs: [
        [0.10, 0.10, 0.55, "rgba(70, 160, 255, 0.55)"],
        [0.90, 0.08, 0.50, "rgba(160, 120, 255, 0.45)"],
        [0.80, 0.88, 0.55, "rgba(60, 210, 230, 0.40)"],
        [0.15, 0.85, 0.50, "rgba(255, 150, 200, 0.40)"],
        [0.52, 0.45, 0.35, "rgba(255, 210, 140, 0.22)"],
      ],
      tintTop: [1.0, 1.0, 1.0],
      tintBottom: [0.86, 0.9, 0.97],
      tint: 0.42,
      centerAlpha: 0.55,
    },
    dark: {
      base: ["#0b101d", "#070a11", "#0c1220"],
      blobs: [
        [0.12, 0.10, 0.55, "rgba(10, 132, 255, 0.42)"],
        [0.88, 0.10, 0.50, "rgba(94, 92, 230, 0.40)"],
        [0.80, 0.86, 0.55, "rgba(48, 176, 199, 0.30)"],
        [0.16, 0.86, 0.50, "rgba(191, 90, 242, 0.30)"],
        [0.50, 0.50, 0.35, "rgba(255, 120, 90, 0.10)"],
      ],
      tintTop: [0.16, 0.19, 0.27],
      tintBottom: [0.07, 0.09, 0.14],
      tint: 0.4,
      centerAlpha: 0.5,
    },
  };

  function palette() { return PALETTES[isDark() ? "dark" : "light"]; }

  function paintWallpaper() {
    if (!wallpaper) {
      wallpaper = document.createElement("canvas");
      wallpaper.id = "lg-wallpaper";
      wallpaper.setAttribute("aria-hidden", "true");
      document.body.prepend(wallpaper);
    }
    const w = Math.max(1, window.innerWidth), h = Math.max(1, window.innerHeight);
    wallpaper.width = w;
    wallpaper.height = h;
    const p = palette();
    const ctx = wallpaper.getContext("2d");
    const lin = ctx.createLinearGradient(0, 0, w * 0.35, h);
    lin.addColorStop(0, p.base[0]);
    lin.addColorStop(0.55, p.base[1]);
    lin.addColorStop(1, p.base[2]);
    ctx.fillStyle = lin;
    ctx.fillRect(0, 0, w, h);
    const span = Math.max(w, h);
    for (const [x, y, r, color] of p.blobs) {
      const g = ctx.createRadialGradient(x * w, y * h, 0, x * w, y * h, r * span);
      g.addColorStop(0, color);
      g.addColorStop(1, "rgba(0, 0, 0, 0)");
      ctx.fillStyle = g;
      ctx.fillRect(0, 0, w, h);
    }
    wallpaperVersion++;
    root.classList.add("lg-on");
    kick();
  }

  // ------------------------------------------------------------ shader
  const VS = `
    attribute vec2 a_position;
    attribute vec2 a_texcoord;
    varying vec2 v_texcoord;
    void main() {
      gl_Position = vec4(a_position, 0, 1);
      v_texcoord = a_texcoord;
    }`;

  // Inti refraksi + blur sama dengan liquid-glass-js. Perubahan: tint per tema,
  // sorotan tepi atas, u_centerAlpha, dan scrollY dihapus (wallpaper tetap).
  const FS = `
    precision mediump float;
    uniform sampler2D u_image;
    uniform vec2 u_resolution;
    uniform vec2 u_textureSize;
    uniform float u_blurRadius;
    uniform float u_borderRadius;
    uniform vec2 u_containerPosition;
    uniform float u_edgeIntensity;
    uniform float u_rimIntensity;
    uniform float u_edgeDistance;
    uniform float u_rimDistance;
    uniform float u_cornerBoost;
    uniform float u_rippleEffect;
    uniform float u_tintOpacity;
    uniform vec3 u_tintTop;
    uniform vec3 u_tintBottom;
    uniform float u_centerAlpha;
    varying vec2 v_texcoord;

    float roundedRectDistance(vec2 coord, vec2 size, float radius) {
      vec2 center = size * 0.5;
      vec2 pixelCoord = coord * size;
      vec2 toCorner = abs(pixelCoord - center) - (center - radius);
      float outsideCorner = length(max(toCorner, 0.0));
      float insideCorner = min(max(toCorner.x, toCorner.y), 0.0);
      return (outsideCorner + insideCorner - radius);
    }

    bool isPill(vec2 size, float radius) {
      return abs(radius - size.y * 0.5) < 2.0 && size.x > size.y + 4.0;
    }

    float pillDistance(vec2 coord, vec2 size, float radius) {
      vec2 center = size * 0.5;
      vec2 pixelCoord = coord * size;
      vec2 capsuleStart = vec2(radius, center.y);
      vec2 capsuleEnd = vec2(size.x - radius, center.y);
      vec2 capsuleAxis = capsuleEnd - capsuleStart;
      if (length(capsuleAxis) > 0.0) {
        float t = clamp(dot(pixelCoord - capsuleStart, capsuleAxis) / dot(capsuleAxis, capsuleAxis), 0.0, 1.0);
        return length(pixelCoord - (capsuleStart + t * capsuleAxis)) - radius;
      }
      return length(pixelCoord - center) - radius;
    }

    void main() {
      vec2 coord = v_texcoord;
      vec2 pagePixel = u_containerPosition + (coord - 0.5) * u_resolution;
      vec2 textureCoord = pagePixel / u_textureSize;

      bool pill = isPill(u_resolution, u_borderRadius);
      float distFromEdgeShape;
      vec2 shapeNormal;
      if (pill) {
        distFromEdgeShape = -pillDistance(coord, u_resolution, u_borderRadius);
        vec2 pixelCoord = coord * u_resolution;
        vec2 capsuleStart = vec2(u_borderRadius, 0.5 * u_resolution.y);
        vec2 capsuleEnd = vec2(u_resolution.x - u_borderRadius, 0.5 * u_resolution.y);
        vec2 capsuleAxis = capsuleEnd - capsuleStart;
        float t = clamp(dot(pixelCoord - capsuleStart, capsuleAxis) / max(dot(capsuleAxis, capsuleAxis), 0.0001), 0.0, 1.0);
        vec2 normalDir = pixelCoord - (capsuleStart + t * capsuleAxis);
        shapeNormal = length(normalDir) > 0.0 ? normalize(normalDir) : vec2(0.0, 1.0);
      } else {
        distFromEdgeShape = -roundedRectDistance(coord, u_resolution, u_borderRadius);
        shapeNormal = normalize(coord - vec2(0.5, 0.5));
      }
      distFromEdgeShape = max(distFromEdgeShape, 0.0);

      float minDim = min(u_resolution.x, u_resolution.y);
      float distFromEdge = distFromEdgeShape / minDim;
      float edgeIntensity = exp(-distFromEdgeShape * u_edgeDistance);
      float rimIntensity = exp(-distFromEdgeShape * u_rimDistance);
      vec2 baseRefraction = shapeNormal * (edgeIntensity * u_edgeIntensity + rimIntensity * u_rimIntensity);

      float cornerDistance = max(min(coord.x, 1.0 - coord.x), min(coord.y, 1.0 - coord.y));
      vec2 cornerRefraction = shapeNormal * exp(-cornerDistance * minDim * 0.3) * u_cornerBoost;

      vec2 perpendicular = vec2(-shapeNormal.y, shapeNormal.x);
      vec2 textureRefraction = perpendicular * sin(distFromEdge * 25.0) * u_rippleEffect * rimIntensity;

      textureCoord += baseRefraction + cornerRefraction + textureRefraction;

      // Gaussian blur
      vec4 color = vec4(0.0);
      vec2 blurStep = (1.0 / u_textureSize) * (u_blurRadius / 2.0);
      float sigma = u_blurRadius / 2.0;
      float totalWeight = 0.0;
      for (float i = -6.0; i <= 6.0; i += 1.0) {
        for (float j = -6.0; j <= 6.0; j += 1.0) {
          float d = length(vec2(i, j));
          if (d > 6.0) continue;
          float weight = exp(-(d * d) / (2.0 * sigma * sigma));
          color += texture2D(u_image, textureCoord + vec2(i, j) * blurStep) * weight;
          totalWeight += weight;
        }
      }
      color /= totalWeight;

      vec3 tinted = mix(color.rgb, mix(u_tintTop, u_tintBottom, coord.y), u_tintOpacity);
      // Sorotan tipis di tepi atas, seperti kaca yang terkena cahaya.
      tinted = mix(tinted, vec3(1.0), exp(-distFromEdgeShape * 0.9) * (1.0 - coord.y) * 0.35);

      float maskDistance = pill ? pillDistance(coord, u_resolution, u_borderRadius)
                                : roundedRectDistance(coord, u_resolution, u_borderRadius);
      float mask = 1.0 - smoothstep(-1.0, 1.0, maskDistance);
      float alpha = mix(u_centerAlpha, 1.0, clamp(edgeIntensity * 1.2, 0.0, 1.0)) * mask;
      gl_FragColor = vec4(tinted * alpha, alpha);
    }`;

  // Nilai bawaan liquid-glass-js; bisa ditimpa lewat window.glassControls.
  const CONTROLS = {
    blurRadius: 5.0,
    edgeIntensity: 0.01,
    rimIntensity: 0.05,
    edgeDistance: 0.15,
    rimDistance: 0.8,
    cornerBoost: 0.02,
    rippleEffect: 0.1,
  };

  function compile(gl, type, src) {
    const s = gl.createShader(type);
    gl.shaderSource(s, src);
    gl.compileShader(s);
    if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) {
      console.error("liquid-glass shader:", gl.getShaderInfoLog(s));
      return null;
    }
    return s;
  }

  // ------------------------------------------------------------ surface
  class Surface {
    constructor(el) {
      this.el = el;
      this.canvas = document.createElement("canvas");
      this.canvas.className = "lg-canvas";
      this.canvas.setAttribute("aria-hidden", "true");
      this.gl = this.canvas.getContext("webgl", { premultipliedAlpha: true, antialias: false });
      this.key = "";
      this.texVersion = -1;
      if (!this.gl || !this.setup()) { this.gl = null; return; }
      el.prepend(this.canvas);
      el.classList.add("lg-surface");
      this.ro = new ResizeObserver(() => kick());
      this.ro.observe(el);
    }

    setup() {
      const gl = this.gl;
      const vs = compile(gl, gl.VERTEX_SHADER, VS), fs = compile(gl, gl.FRAGMENT_SHADER, FS);
      if (!vs || !fs) return false;
      const prog = gl.createProgram();
      gl.attachShader(prog, vs);
      gl.attachShader(prog, fs);
      gl.linkProgram(prog);
      if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) return false;
      gl.useProgram(prog);
      const attr = (data, name) => {
        gl.bindBuffer(gl.ARRAY_BUFFER, gl.createBuffer());
        gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(data), gl.STATIC_DRAW);
        const loc = gl.getAttribLocation(prog, name);
        gl.enableVertexAttribArray(loc);
        gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);
      };
      attr([-1, -1, 1, -1, -1, 1, -1, 1, 1, -1, 1, 1], "a_position");
      attr([0, 1, 1, 1, 0, 0, 0, 0, 1, 1, 1, 0], "a_texcoord");
      this.u = {};
      for (const n of ["u_image", "u_resolution", "u_textureSize", "u_blurRadius", "u_borderRadius", "u_containerPosition",
        "u_edgeIntensity", "u_rimIntensity", "u_edgeDistance", "u_rimDistance", "u_cornerBoost", "u_rippleEffect",
        "u_tintOpacity", "u_tintTop", "u_tintBottom", "u_centerAlpha"]) this.u[n] = gl.getUniformLocation(prog, n);
      const c = Object.assign({}, CONTROLS, window.glassControls || {});
      gl.uniform1f(this.u.u_blurRadius, c.blurRadius);
      gl.uniform1f(this.u.u_edgeIntensity, c.edgeIntensity);
      gl.uniform1f(this.u.u_rimIntensity, c.rimIntensity);
      gl.uniform1f(this.u.u_edgeDistance, c.edgeDistance);
      gl.uniform1f(this.u.u_rimDistance, c.rimDistance);
      gl.uniform1f(this.u.u_cornerBoost, c.cornerBoost);
      gl.uniform1f(this.u.u_rippleEffect, c.rippleEffect);
      gl.activeTexture(gl.TEXTURE0);
      gl.bindTexture(gl.TEXTURE_2D, gl.createTexture());
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      gl.uniform1i(this.u.u_image, 0);
      gl.clearColor(0, 0, 0, 0);
      return true;
    }

    // Render hanya jika posisi, ukuran, atau wallpaper berubah.
    update() {
      const gl = this.gl;
      if (!gl || !wallpaper) return;
      const r = this.el.getBoundingClientRect();
      if (r.width < 1 || r.height < 1) return;
      const cs = getComputedStyle(this.el);
      const radius = Math.min(parseFloat(cs.borderTopLeftRadius) || 0, r.width / 2, r.height / 2);
      const key = [r.left, r.top, r.width, r.height, radius].map((v) => Math.round(v * 2)).join(",");
      if (key === this.key && this.texVersion === wallpaperVersion) return;
      if (this.texVersion !== wallpaperVersion) {
        gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, wallpaper);
        gl.uniform2f(this.u.u_textureSize, wallpaper.width, wallpaper.height);
        const p = palette();
        gl.uniform3fv(this.u.u_tintTop, p.tintTop);
        gl.uniform3fv(this.u.u_tintBottom, p.tintBottom);
        gl.uniform1f(this.u.u_tintOpacity, p.tint);
        gl.uniform1f(this.u.u_centerAlpha, p.centerAlpha);
        this.texVersion = wallpaperVersion;
      }
      this.key = key;
      // Kanvas menutupi kotak border; posisi absolut dihitung dari padding box.
      const bl = parseFloat(cs.borderLeftWidth) || 0, bt = parseFloat(cs.borderTopWidth) || 0;
      const st = this.canvas.style;
      st.left = -bl + "px";
      st.top = -bt + "px";
      st.width = r.width + "px";
      st.height = r.height + "px";
      st.borderRadius = radius + "px";
      const dpr = Math.min(window.devicePixelRatio || 1, 2);
      const pw = Math.round(r.width * dpr), ph = Math.round(r.height * dpr);
      if (this.canvas.width !== pw || this.canvas.height !== ph) {
        this.canvas.width = pw;
        this.canvas.height = ph;
      }
      gl.viewport(0, 0, pw, ph);
      gl.uniform2f(this.u.u_resolution, r.width, r.height);
      gl.uniform1f(this.u.u_borderRadius, radius);
      gl.uniform2f(this.u.u_containerPosition, r.left + r.width / 2, r.top + r.height / 2);
      gl.clear(gl.COLOR_BUFFER_BIT);
      gl.drawArrays(gl.TRIANGLES, 0, 6);
    }

    destroy() {
      if (this.ro) this.ro.disconnect();
      const ext = this.gl && this.gl.getExtension("WEBGL_lose_context");
      if (ext) ext.loseContext();
      this.canvas.remove();
      this.el.classList.remove("lg-surface");
      this.gl = null;
    }
  }

  // ------------------------------------------------------------ wiring
  function scan() {
    for (const [el, s] of surfaces) {
      if (!el.isConnected) { s.destroy(); surfaces.delete(el); }
    }
    for (const sel of SELECTORS) {
      document.querySelectorAll(sel).forEach((el) => {
        if (surfaces.has(el)) return;
        const s = new Surface(el);
        if (s.gl) surfaces.set(el, s);
      });
    }
    kick();
  }

  // Jalankan pengecekan per frame selama `ms` setelah ada pemicu, lalu diam.
  let until = 0, running = false;
  function kick(ms = 500) {
    until = Math.max(until, performance.now() + ms);
    if (!running) { running = true; requestAnimationFrame(loop); }
  }
  function loop(now) {
    for (const s of surfaces.values()) s.update();
    if (now < until) requestAnimationFrame(loop);
    else running = false;
  }

  function start() {
    paintWallpaper();
    scan();
    let pending = 0;
    window.addEventListener("resize", () => {
      cancelAnimationFrame(pending);
      pending = requestAnimationFrame(paintWallpaper);
    });
    window.addEventListener("scroll", () => kick(200), { passive: true, capture: true });
    for (const ev of ["transitionrun", "transitionend", "animationstart", "animationend"]) {
      document.addEventListener(ev, () => kick(), true);
    }
    new MutationObserver(paintWallpaper).observe(root, { attributes: true, attributeFilter: ["data-theme"] });
    const dark = mm("(prefers-color-scheme: dark)");
    if (dark && dark.addEventListener) dark.addEventListener("change", paintWallpaper);
    if (reduce && reduce.addEventListener) reduce.addEventListener("change", () => { if (reduce.matches) location.reload(); });
    new MutationObserver(scan).observe(document.body, { childList: true });
  }

  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", start);
  else start();
})();
