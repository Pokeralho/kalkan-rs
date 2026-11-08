// Kalkan Studio — editor visual de temas.
// O canvas é renderizado pelo motor Rust real; aqui ficam seleção, edição,
// camadas, inspector e histórico.
"use strict";

const T = window.__TAURI__;
const invoke = (cmd, args) => T.core.invoke(cmd, args);
const $ = (s, el = document) => el.querySelector(s);
const W = 320, H = 240;

// ───────────────────────── Estado ─────────────────────────
const S = {
  info: null,
  themes: [],
  themeId: null,
  doc: null,            // { meta, background, layers: [...] }
  sel: -1,              // índice da camada selecionada
  bounds: new Map(),    // z(índice) → [x,y,w,h] vindos do motor
  zoom: 3,
  snap: true,
  grid: false,
  dirty: false,
  docVersion: 0,        // incrementa a cada mudança local
  sentVersion: -1,      // versão já enviada ao motor
  undo: [], redo: [],
  clipboard: null,
  drag: null,
  guides: [],
  hover: -1,
  telemetry: {},
  config: null,
};

// ───────────────────────── Definições de widgets ─────────────────────────
const ICONS = {
  text: "T", radial_gauge: "◔", bar: "▬", sparkline: "∿", image: "▣", rect: "□", shape: "◆", cores: "▥",
};
const SHAPE_ICONS = { rect: "□", ellipse: "○", regular: "⬡", star: "★", polygon: "⬠", line: "╱" };

const svgIcon = (d) => `<svg viewBox="0 0 24 24">${d}</svg>`;
const PALETTE = [
  { label: "Texto", icon: svgIcon('<path d="M5 6h14M12 6v13"/>'), make: () => ({ type: "text", text: "Texto {cpu_temp}°", size: 22, x: 160, y: 120, align: "center", valign: "middle", color_hex: "#FFFFFF", font: "segoeui.ttf" }) },
  { label: "Gauge", icon: svgIcon('<path d="M5 17a8 8 0 1 1 14 0"/><path d="M12 13l4-4"/>'), make: () => ({ type: "radial_gauge", metric: "cpu_usage", min: 0, max: 100, center_x: 160, center_y: 120, radius: 48, start_angle: -135, end_angle: 135, stroke_width: 8, color_gradient: ["#22D3EE", "#8B6CFF", "#FF4FD8"], track_color: "#FFFFFF1F", rounded_caps: true, smooth: true }) },
  { label: "Barra", icon: svgIcon('<rect x="3" y="9" width="18" height="6" rx="3"/><path d="M5 12h8"/>'), make: () => ({ type: "bar", metric: "ram_pct", min: 0, max: 100, x: 100, y: 116, width: 120, height: 8, radius: 4, orientation: "horizontal", color_gradient: ["#34D399", "#FBBF24", "#F87171"], track_color: "#FFFFFF1F", smooth: true }) },
  { label: "Gráfico", icon: svgIcon('<path d="M3 17l5-6 4 3 4-7 5 5"/>'), make: () => ({ type: "sparkline", metric: "cpu_usage", x: 60, y: 90, width: 200, height: 60, seconds: 30, min: 0, max: 100, color_hex: "#22D3EE", fill_opacity: 0.25, stroke_width: 1.5 }) },
  { label: "Retângulo", icon: svgIcon('<rect x="4" y="6" width="16" height="12" rx="3"/>'), make: () => shape("rect", { corner_radius: 10, fill: "#8B6CFF55", stroke: "#8B6CFF", stroke_width: 1.5 }) },
  { label: "Elipse", icon: svgIcon('<ellipse cx="12" cy="12" rx="8" ry="6"/>'), make: () => shape("ellipse", { fill_gradient: ["#22D3EE", "#8B6CFF"], gradient_type: "radial" }) },
  { label: "Polígono", icon: svgIcon('<path d="M12 3l8 6-3 10H7L4 9z"/>'), make: () => shape("regular", { sides: 6, fill: "#22D3EE44", stroke: "#22D3EE", stroke_width: 2 }) },
  { label: "Estrela", icon: svgIcon('<path d="M12 3l2.6 5.6 6 .7-4.5 4.1 1.2 6L12 16.5 6.7 19.4l1.2-6L3.4 9.3l6-.7z"/>'), make: () => shape("star", { sides: 5, inner_ratio: 0.45, fill_gradient: ["#FBBF24", "#FF4FD8"], gradient_angle: 90 }) },
  { label: "Livre", icon: svgIcon('<path d="M4 18l3-11 6 4 3-6 4 13z"/>'), make: () => shape("polygon", { points: [[0, 1], [0.15, 0.2], [0.45, 0.55], [0.7, 0], [1, 1]], fill_gradient: ["#34D39988", "#22D3EE22"], gradient_angle: 90, stroke: "#34D399", stroke_width: 1.5 }) },
  { label: "Linha", icon: svgIcon('<path d="M4 18L20 6"/>'), make: () => shape("line", { points: [[0, 0.5], [1, 0.5]], height: 10, stroke: "#FFFFFF66", stroke_width: 2 }) },
  { label: "Imagem", icon: svgIcon('<rect x="3" y="5" width="18" height="14" rx="2"/><circle cx="9" cy="10" r="2"/><path d="M21 17l-5-5-8 7"/>'), make: null, action: () => addImage() },
  { label: "Painel", icon: svgIcon('<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 9h18"/>'), make: () => ({ type: "rect", x: 10, y: 10, width: 140, height: 80, radius: 10, color_hex: "#00000088" }) },
  { label: "Núcleos", icon: svgIcon('<path d="M5 20V12M9 20V6M13 20V14M17 20V9M21 20V4"/>'), make: () => ({ type: "cores", x: 40, y: 140, width: 240, height: 60, columns: 0, gap: 2, orientation: "vertical", radius: 2, color_gradient: ["#22D3EE", "#8B6CFF", "#FF4FD8"], track_color: "#FFFFFF14" }) },
  { label: "Floco", icon: svgIcon('<path d="M12 3v18M4.2 7.5l15.6 9M4.2 16.5l15.6-9"/>'), make: () => shape("line", { x: 110, y: 70, width: 100, height: 100, points: [[0.5, 0.5], [0.5, 0.05]], paths: [[[0.37, 0.11], [0.5, 0.24], [0.63, 0.11]], [[0.41, 0.28], [0.5, 0.37], [0.59, 0.28]]], repeat: 6, stroke: "#38E1F0", stroke_width: 6, glow: "#38E1F0", glow_radius: 7 }) },
  { label: "Mandala", icon: svgIcon('<circle cx="12" cy="12" r="3"/><path d="M12 2c3 3 3 7 0 7s-3-4 0-7zM12 22c3-3 3-7 0-7s-3 4 0 7z"/>'), make: () => shape("ellipse", { x: 130, y: 60, width: 60, height: 120, repeat: 8, fill_gradient: ["#FF4FD855", "#8B6CFF11"], gradient_type: "radial", stroke: "#FF4FD8", stroke_width: 1, animation: "spin", animation_speed: 0.15 }) },
];

function shape(kind, extra) {
  return Object.assign({
    type: "shape", kind, x: 110, y: 70, width: 100, height: 100, rotation: 0,
    fill: "#FFFFFF", fill_gradient: [], gradient_type: "linear", gradient_angle: 0,
    stroke_width: 0, opacity: 1, sides: 5, inner_ratio: 0.5, corner_radius: 0, points: [],
    min: 0, max: 100, fill_direction: 270,
  }, extra);
}

const PRESETS = [
  { name: "Relógio", desc: "hora + data", layers: () => [
    { type: "text", text: "{time:%H:%M}", font: "segoeuil.ttf", size: 64, x: 160, y: 96, align: "center", valign: "middle", color_hex: "#FFFFFF" },
    { type: "text", text: "{date:%d/%m/%Y}", font: "segoeui.ttf", size: 14, x: 160, y: 140, align: "center", valign: "middle", color_hex: "#9AA4B8" },
  ]},
  { name: "Card CPU", desc: "gauge + temp", layers: () => [
    { type: "rect", x: 16, y: 40, width: 128, height: 160, radius: 14, color_hex: "#0B0B14CC" },
    PALETTE[1].make(),
    { type: "text", text: "{cpu_temp}°", font: "bahnschrift.ttf", size: 30, x: 80, y: 116, align: "center", valign: "middle", color_hex: "#FFFFFF" },
    { type: "text", text: "CPU {cpu_usage:.0}%", font: "bahnschrift.ttf", size: 12, x: 80, y: 176, align: "center", valign: "middle", color_hex: "#8B8BA6" },
  ].map((l, i) => i === 1 ? Object.assign(l, { center_x: 80, center_y: 116, radius: 46, metric: "cpu_temp", min: 30, max: 95 }) : l) },
  { name: "Card GPU", desc: "gauge + temp", layers: () => [
    { type: "rect", x: 176, y: 40, width: 128, height: 160, radius: 14, color_hex: "#0B0B14CC" },
    Object.assign(PALETTE[1].make(), { center_x: 240, center_y: 116, radius: 46, metric: "gpu_temp", min: 30, max: 90, color_gradient: ["#34D399", "#FBBF24", "#F87171"] }),
    { type: "text", text: "{gpu_temp}°", font: "bahnschrift.ttf", size: 30, x: 240, y: 116, align: "center", valign: "middle", color_hex: "#FFFFFF" },
    { type: "text", text: "GPU {gpu_usage:.0}%", font: "bahnschrift.ttf", size: 12, x: 240, y: 176, align: "center", valign: "middle", color_hex: "#8B8BA6" },
  ]},
  { name: "Rede", desc: "↓↑ + gráfico", layers: () => [
    { type: "sparkline", metric: "net_down_kb", x: 16, y: 170, width: 288, height: 56, seconds: 60, min: 0, color_hex: "#6EC1FF", fill_opacity: 0.25, stroke_width: 1.5 },
    { type: "text", text: "↓ {net_down}", font: "segoeui.ttf", size: 14, x: 16, y: 150, color_hex: "#6EC1FF" },
    { type: "text", text: "↑ {net_up}", font: "segoeui.ttf", size: 14, x: 304, y: 150, align: "right", color_hex: "#FFB36E" },
  ]},
  { name: "Música", desc: "título + artista", layers: () => [
    { type: "text", text: "{media_icon}  {media_title}", font: "segoeui.ttf", size: 16, x: 16, y: 200, max_width: 288, color_hex: "#FFFFFF" },
    { type: "text", text: "{media_artist}", font: "segoeui.ttf", size: 12, x: 16, y: 220, max_width: 288, color_hex: "#8B8BA6" },
  ]},
  { name: "Medidor líquido", desc: "forma que enche", layers: () => [
    shape("regular", { sides: 6, x: 110, y: 70, width: 100, height: 100, rotation: 30, fill: "#FFFFFF10", stroke: "#22D3EE", stroke_width: 2 }),
    shape("regular", { sides: 6, x: 110, y: 70, width: 100, height: 100, rotation: 30, fill_gradient: ["#22D3EE", "#8B6CFF"], gradient_angle: 90, metric: "cpu_usage", opacity: 0.9 }),
    { type: "text", text: "{cpu_usage:.0}%", font: "bahnschrift.ttf", size: 22, x: 160, y: 120, align: "center", valign: "middle", color_hex: "#FFFFFF", shadow: "#000000AA" },
  ]},
];

// ───────────────────────── Utilitários ─────────────────────────
const clone = (o) => JSON.parse(JSON.stringify(o));
const round = (v, p = 1) => Math.round(v * 10 ** p) / 10 ** p;
const clamp = (v, a, b) => Math.min(b, Math.max(a, v));
const esc = (s) => String(s ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));

function toast(msg, isErr = false) {
  const t = $("#toast");
  t.textContent = msg;
  t.className = "toast show" + (isErr ? " err" : "");
  clearTimeout(toast._t);
  toast._t = setTimeout(() => (t.className = "toast" + (isErr ? " err" : "")), isErr ? 4200 : 1800);
}

function ask(title, value = "") {
  const dlg = $("#dlg");
  $("#dlgTitle").textContent = title;
  const inp = $("#dlgInput");
  inp.value = value;
  return new Promise((res) => {
    dlg.onclose = () => res(dlg.returnValue === "ok" ? inp.value.trim() : null);
    dlg.showModal();
    inp.select();
  });
}

function layerName(l) {
  switch (l.type) {
    case "text": return l.text.length > 28 ? l.text.slice(0, 28) + "…" : l.text;
    case "radial_gauge": return `Gauge · ${l.metric}`;
    case "bar": return `Barra · ${l.metric}`;
    case "sparkline": return `Gráfico · ${l.metric}`;
    case "image": return `Imagem · ${l.path}`;
    case "rect": return "Painel";
    case "cores": return "Núcleos da CPU";
    case "shape": {
      const n = { rect: "Retângulo", ellipse: "Elipse", regular: `Polígono ${l.sides}`, star: "Estrela", polygon: "Forma livre", line: "Linha" }[l.kind] || l.kind;
      return l.metric ? `${n} · ${l.metric}` : n;
    }
  }
  return l.type;
}

// Caixa de edição de cada tipo (texto/gauge usam a caixa calculada pelo motor).
function boxOf(i) {
  const l = S.doc.layers[i];
  switch (l.type) {
    case "rect": case "shape": case "bar": case "sparkline": case "cores":
      return [l.x, l.y, l.width, l.height];
    case "radial_gauge": {
      const r = l.radius + l.stroke_width / 2;
      return [l.center_x - r, l.center_y - r, 2 * r, 2 * r];
    }
    default:
      return S.bounds.get(i) || [l.x || 0, l.y || 0, 10, 10];
  }
}

function moveLayer(l, dx, dy) {
  if (l.type === "radial_gauge") { l.center_x = round(l.center_x + dx); l.center_y = round(l.center_y + dy); }
  else { l.x = round(l.x + dx); l.y = round(l.y + dy); }
}

// ───────────────────────── Histórico ─────────────────────────
function snapshot() { return JSON.stringify({ doc: S.doc, sel: S.sel }); }
function commit() {
  S.undo.push(S._before ?? snapshot());
  if (S.undo.length > 200) S.undo.shift();
  S.redo = [];
  S._before = null;
  markDirty();
}
function beginChange() { if (S._before == null) S._before = snapshot(); }
function change(fn, { history = true } = {}) {
  if (history) beginChange();
  fn();
  S.docVersion++;
  if (history) commit();
  refreshPanels();
}
function restore(snap) {
  const o = JSON.parse(snap);
  S.doc = o.doc; S.sel = o.sel;
  S.docVersion++;
  markDirty();
  refreshPanels();
}
function undo() { if (!S.undo.length) return; S.redo.push(snapshot()); restore(S.undo.pop()); }
function redo() { if (!S.redo.length) return; S.undo.push(snapshot()); restore(S.redo.pop()); }
function markDirty(v = true) {
  S.dirty = v;
  $("#dirtyDot").classList.toggle("on", v);
  $("#btnUndo").disabled = !S.undo.length;
  $("#btnRedo").disabled = !S.redo.length;
}

// ───────────────────────── Render loop ─────────────────────────
const ctx2d = $("#screen").getContext("2d");
const img = ctx2d.createImageData(W, H);
let frames = 0, fpsT = performance.now(), busy = false, lastErr = "";

async function frame() {
  requestAnimationFrame(frame);
  if (busy || !S.doc) return;
  const now = performance.now();
  if (now - (frame._last || 0) < 33) return; // ~30 fps
  frame._last = now;
  busy = true;
  try {
    if (S.sentVersion !== S.docVersion) {
      const v = S.docVersion;
      try {
        await invoke("update_doc", { doc: S.doc });
        if (lastErr) { lastErr = ""; }
      } catch (e) {
        if (String(e) !== lastErr) { lastErr = String(e); toast(lastErr, true); }
      }
      S.sentVersion = v;
    }
    const buf = await invoke("render");
    const dv = new DataView(buf);
    const n = dv.getUint32(0, true);
    S.bounds.clear();
    let o = 4;
    for (let i = 0; i < n; i++, o += 20) {
      S.bounds.set(dv.getFloat32(o, true), [dv.getFloat32(o + 4, true), dv.getFloat32(o + 8, true), dv.getFloat32(o + 12, true), dv.getFloat32(o + 16, true)]);
    }
    img.data.set(new Uint8Array(buf, o, W * H * 4));
    ctx2d.putImageData(img, 0, 0);
    drawOverlay();
    frames++;
    if (now - fpsT > 1000) { $("#stFps").textContent = `${frames} fps`; frames = 0; fpsT = now; }
  } catch (e) {
    console.error(e);
  } finally { busy = false; }
}

// ───────────────────────── Overlay (seleção, guias, pontos) ─────────────────────────
const ov = $("#overlay");
const NS = "http://www.w3.org/2000/svg";
function el(tag, attrs, parent = ov) {
  const e = document.createElementNS(NS, tag);
  for (const k in attrs) e.setAttribute(k, attrs[k]);
  parent.appendChild(e);
  return e;
}

function drawOverlay() {
  ov.innerHTML = "";
  if (!S.doc) return;
  const hs = 7 / S.zoom; // tamanho do handle em px de tela
  if (S.grid) {
    const g = el("g", { class: "grid" });
    for (let x = 10; x < W; x += 10) el("line", { x1: x, y1: 0, x2: x, y2: H }, g);
    for (let y = 10; y < H; y += 10) el("line", { x1: 0, y1: y, x2: W, y2: y }, g);
  }
  if (S.hover >= 0 && S.hover !== S.sel && S.doc.layers[S.hover]) {
    const [x, y, w, h] = boxOf(S.hover);
    el("rect", { x, y, width: w, height: h, class: "hover-box" });
  }
  const l = S.doc.layers[S.sel];
  if (l) {
    const [x, y, w, h] = boxOf(S.sel);
    el("rect", { x, y, width: w, height: h, class: "sel-box" });
    if (resizable(l)) {
      for (const [hx, hy, id] of handlesOf(x, y, w, h)) {
        el("rect", { x: hx - hs / 2, y: hy - hs / 2, width: hs, height: hs, class: "handle", "data-h": id, style: `cursor:${id}-resize` });
      }
    }
    if (l.type === "shape" && (l.kind === "polygon" || l.kind === "line")) {
      const pts = l.points.map(([px, py]) => [l.x + px * l.width, l.y + py * l.height]);
      el("polyline", { points: pts.map((p) => p.join(",")).join(" ") + (l.kind === "polygon" && pts.length ? " " + pts[0].join(",") : ""), class: "poly-edit" });
      pts.forEach(([px, py], i) => el("circle", { cx: px, cy: py, r: 4.5 / S.zoom, class: "pt", "data-pt": i }));
    }
  }
  for (const g of S.guides) {
    if (g.x != null) el("line", { x1: g.x, y1: -20, x2: g.x, y2: H + 20, class: "guide" });
    else el("line", { x1: -20, y1: g.y, x2: W + 20, y2: g.y, class: "guide" });
  }
}

const resizable = (l) => ["rect", "shape", "bar", "sparkline", "image", "radial_gauge", "text", "cores"].includes(l.type);
function handlesOf(x, y, w, h) {
  return [
    [x, y, "nw"], [x + w / 2, y, "n"], [x + w, y, "ne"], [x + w, y + h / 2, "e"],
    [x + w, y + h, "se"], [x + w / 2, y + h, "s"], [x, y + h, "sw"], [x, y + h / 2, "w"],
  ];
}

// ───────────────────────── Interação no canvas ─────────────────────────
function toCanvas(ev) {
  const r = ov.getBoundingClientRect();
  return [(ev.clientX - r.left) / r.width * W, (ev.clientY - r.top) / r.height * H];
}

function hitTest(px, py) {
  for (let i = S.doc.layers.length - 1; i >= 0; i--) {
    const l = S.doc.layers[i];
    if (l._hidden || l._locked) continue;
    const [x, y, w, h] = boxOf(i);
    if (px >= x - 2 && px <= x + w + 2 && py >= y - 2 && py <= y + h + 2) return i;
  }
  return -1;
}

ov.addEventListener("pointerdown", (ev) => {
  if (!S.doc) return;
  try { ov.setPointerCapture(ev.pointerId); } catch {}
  const [px, py] = toCanvas(ev);
  const t = ev.target;
  const l = S.doc.layers[S.sel];

  // Ponto de forma livre
  if (t.dataset.pt != null && l) {
    const i = +t.dataset.pt;
    if (ev.altKey) {
      const min = l.kind === "line" ? 2 : 3;
      if (l.points.length > min) change(() => l.points.splice(i, 1));
      return;
    }
    beginChange();
    S.drag = { mode: "point", i, start: [px, py], orig: clone(l.points[i]) };
    return;
  }
  // Handle de redimensionamento
  if (t.dataset.h && l) {
    beginChange();
    S.drag = { mode: "resize", h: t.dataset.h, start: [px, py], box: boxOf(S.sel), orig: clone(l) };
    return;
  }
  const i = hitTest(px, py);
  select(i);
  if (i >= 0) {
    beginChange();
    S.drag = { mode: "move", start: [px, py], orig: clone(S.doc.layers[i]), box: boxOf(i), moved: false };
  }
});

ov.addEventListener("dblclick", (ev) => {
  const l = S.doc?.layers[S.sel];
  if (!l || l.type !== "shape" || !(l.kind === "polygon" || l.kind === "line")) return;
  const [px, py] = toCanvas(ev);
  // Insere um ponto no segmento mais próximo.
  const pts = l.points.map(([x, y]) => [l.x + x * l.width, l.y + y * l.height]);
  const n = l.kind === "polygon" ? pts.length : pts.length - 1;
  let best = { d: Infinity, at: pts.length };
  for (let k = 0; k < n; k++) {
    const [ax, ay] = pts[k], [bx, by] = pts[(k + 1) % pts.length];
    const vx = bx - ax, vy = by - ay, len = vx * vx + vy * vy || 1;
    const t = clamp(((px - ax) * vx + (py - ay) * vy) / len, 0, 1);
    const d = Math.hypot(ax + vx * t - px, ay + vy * t - py);
    if (d < best.d) best = { d, at: k + 1 };
  }
  change(() => l.points.splice(best.at, 0, [round((px - l.x) / (l.width || 1), 3), round((py - l.y) / (l.height || 1), 3)]));
});

ov.addEventListener("pointermove", (ev) => {
  if (!S.doc) return;
  const [px, py] = toCanvas(ev);
  $("#stCursor").textContent = `x ${Math.round(px)} · y ${Math.round(py)}`;
  const d = S.drag;
  if (!d) {
    const h = hitTest(px, py);
    if (h !== S.hover) { S.hover = h; drawOverlay(); }
    return;
  }
  const l = S.doc.layers[S.sel];
  let dx = px - d.start[0], dy = py - d.start[1];
  S.guides = [];

  if (d.mode === "move") {
    if (!d.moved && Math.hypot(dx, dy) < 0.6) return;
    d.moved = true;
    if (ev.shiftKey) { if (Math.abs(dx) > Math.abs(dy)) dy = 0; else dx = 0; }
    [dx, dy] = snapMove(d.box, dx, dy);
    Object.assign(l, clone(d.orig));
    moveLayer(l, dx, dy);
  } else if (d.mode === "resize") {
    resizeLayer(l, d, dx, dy, ev.shiftKey);
  } else if (d.mode === "point") {
    l.points[d.i] = [round(d.orig[0] + dx / (l.width || 1), 3), round(d.orig[1] + dy / (l.height || 1), 3)];
  }
  S.docVersion++;
  updateGeometryInputs();
});

ov.addEventListener("pointerup", () => {
  if (S.drag) {
    if (S.drag.mode !== "move" || S.drag.moved) commit(); else S._before = null;
    S.drag = null;
    S.guides = [];
    refreshPanels();
  }
});
ov.addEventListener("pointerleave", () => { if (!S.drag && S.hover !== -1) { S.hover = -1; drawOverlay(); } });

function snapMove(box, dx, dy) {
  const g = S.snap ? 4 / S.zoom * 1.5 : 0;
  let [x, y, w, h] = box;
  let nx = x + dx, ny = y + dy;
  if (S.snap) {
    const xs = [0, W / 2, W], ys = [0, H / 2, H];
    S.doc.layers.forEach((_, i) => {
      if (i === S.sel || S.doc.layers[i]._hidden) return;
      const [bx, by, bw, bh] = boxOf(i);
      xs.push(bx, bx + bw / 2, bx + bw); ys.push(by, by + bh / 2, by + bh);
    });
    const tryAxis = (pos, size, cands, key) => {
      let best = null;
      for (const off of [0, size / 2, size]) for (const c of cands) {
        const dd = c - (pos + off);
        if (Math.abs(dd) <= Math.max(g, 2) && (!best || Math.abs(dd) < Math.abs(best.dd))) best = { dd, c };
      }
      if (best) { S.guides.push({ [key]: best.c }); return pos + best.dd; }
      return Math.round(pos);
    };
    nx = tryAxis(nx, w, xs, "x");
    ny = tryAxis(ny, h, ys, "y");
  } else { nx = Math.round(nx); ny = Math.round(ny); }
  return [nx - x, ny - y];
}

function resizeLayer(l, d, dx, dy, keepRatio) {
  let [x, y, w, h] = d.box;
  const id = d.h;
  if (id.includes("e")) w += dx;
  if (id.includes("s")) h += dy;
  if (id.includes("w")) { x += dx; w -= dx; }
  if (id.includes("n")) { y += dy; h -= dy; }
  if (keepRatio || l.type === "radial_gauge" || l.type === "text") {
    const r = d.box[2] / (d.box[3] || 1);
    if (id === "n" || id === "s") w = h * r; else h = w / r;
    if (id.includes("n")) y = d.box[1] + d.box[3] - h;
    if (id.includes("w")) x = d.box[0] + d.box[2] - w;
  }
  w = Math.max(2, w); h = Math.max(2, h);
  const o = d.orig;
  switch (l.type) {
    case "radial_gauge": {
      const sw = o.stroke_width;
      l.radius = round(Math.max(4, Math.min(w, h) / 2 - sw / 2));
      l.center_x = round(x + w / 2); l.center_y = round(y + h / 2);
      break;
    }
    case "text":
      l.size = round(Math.max(4, o.size * h / (d.box[3] || 1)));
      break;
    case "image":
      l.x = round(x); l.y = round(y); l.width = Math.round(w); l.height = Math.round(h);
      break;
    default:
      l.x = round(x); l.y = round(y); l.width = round(w); l.height = round(h);
  }
}

// ───────────────────────── Teclado ─────────────────────────
window.addEventListener("keydown", (ev) => {
  const typing = ["INPUT", "TEXTAREA", "SELECT"].includes(document.activeElement?.tagName);
  const ctrl = ev.ctrlKey || ev.metaKey;
  if (ctrl && ev.key.toLowerCase() === "s") { ev.preventDefault(); save(); return; }
  if (typing) return;
  if (ctrl && ev.key.toLowerCase() === "z") { ev.preventDefault(); ev.shiftKey ? redo() : undo(); return; }
  if (ctrl && ev.key.toLowerCase() === "y") { ev.preventDefault(); redo(); return; }
  const l = S.doc?.layers[S.sel];
  if (ctrl && ev.key.toLowerCase() === "c" && l) { S.clipboard = clone(l); toast("Camada copiada"); return; }
  if (ctrl && ev.key.toLowerCase() === "v" && S.clipboard) { const c = clone(S.clipboard); moveLayer(c, 8, 8); addLayers([c]); return; }
  if (ctrl && ev.key.toLowerCase() === "d" && l) { ev.preventDefault(); duplicate(); return; }
  if (!l) return;
  const step = ev.shiftKey ? 10 : 1;
  const arrows = { ArrowLeft: [-step, 0], ArrowRight: [step, 0], ArrowUp: [0, -step], ArrowDown: [0, step] };
  if (arrows[ev.key]) { ev.preventDefault(); change(() => moveLayer(l, ...arrows[ev.key])); return; }
  if (ev.key === "Delete" || ev.key === "Backspace") { removeLayer(S.sel); return; }
  if (ev.key === "]") reorder(S.sel, S.sel + 1);
  if (ev.key === "[") reorder(S.sel, S.sel - 1);
  if (ev.key === "Escape") select(-1);
});

// ───────────────────────── Camadas ─────────────────────────
function select(i) {
  S.sel = i;
  refreshPanels();
}

function addLayers(list) {
  change(() => {
    const at = S.sel >= 0 ? S.sel + 1 : S.doc.layers.length;
    S.doc.layers.splice(at, 0, ...list);
    S.sel = at + list.length - 1;
  });
  switchTab("props");
}

function removeLayer(i) {
  if (i < 0) return;
  change(() => { S.doc.layers.splice(i, 1); S.sel = Math.min(i, S.doc.layers.length - 1); });
}

function duplicate() {
  const c = clone(S.doc.layers[S.sel]);
  moveLayer(c, 8, 8);
  addLayers([c]);
}

function reorder(from, to) {
  if (to < 0 || to >= S.doc.layers.length || from === to) return;
  change(() => {
    const [l] = S.doc.layers.splice(from, 1);
    S.doc.layers.splice(to, 0, l);
    S.sel = to;
  });
}

function renderLayers() {
  const ul = $("#layers");
  ul.innerHTML = "";
  $("#layerCount").textContent = S.doc ? S.doc.layers.length : "";
  if (!S.doc) return;
  // Lista exibida de cima (frente) para baixo (fundo).
  for (let i = S.doc.layers.length - 1; i >= 0; i--) {
    const l = S.doc.layers[i];
    const li = document.createElement("li");
    li.className = "layer" + (i === S.sel ? " sel" : "") + (l._hidden ? " off" : "");
    li.draggable = true;
    li.innerHTML = `<span class="ico">${l.type === "shape" ? SHAPE_ICONS[l.kind] || "◆" : ICONS[l.type]}</span>
      <span class="name">${esc(layerName(l))}</span>
      <button class="mini ${l._locked ? "on" : ""}" data-act="lock" title="Travar">${l._locked ? "🔒" : "🔓"}</button>
      <button class="mini ${l._hidden ? "on" : ""}" data-act="eye" title="Ocultar">${l._hidden ? "◌" : "●"}</button>`;
    li.onclick = (e) => {
      const act = e.target.dataset.act;
      if (act === "eye") change(() => (l._hidden = !l._hidden));
      else if (act === "lock") change(() => (l._locked = !l._locked));
      else select(i);
    };
    li.ondragstart = (e) => e.dataTransfer.setData("text/plain", String(i));
    li.ondragover = (e) => { e.preventDefault(); li.classList.add("drag-over"); };
    li.ondragleave = () => li.classList.remove("drag-over");
    li.ondrop = (e) => { e.preventDefault(); reorder(+e.dataTransfer.getData("text/plain"), i); };
    ul.appendChild(li);
  }
}

// ───────────────────────── Inspector ─────────────────────────
const METRIC_GROUPS = () => S.info?.numeric_keys || [];

function seg(options, value, onPick) {
  const d = document.createElement("div");
  d.className = "seg";
  for (const [v, label] of options) {
    const b = document.createElement("button");
    b.textContent = label;
    b.className = v === value ? "on" : "";
    b.onclick = () => onPick(v);
    d.appendChild(b);
  }
  return d;
}

function hexToParts(hex) {
  // Referências à paleta ($nome) são resolvidas só para exibição.
  for (let i = 0; i < 3 && typeof hex === "string" && hex.startsWith("$"); i++) hex = S.doc?.palette?.[hex.slice(1)] || "#FFFFFF";
  let h = String(hex || "#FFFFFF").trim().replace(/^#/, "");
  // Só hexadecimal válido chega ao DOM (temas de terceiros não injetam HTML/CSS).
  if (!/^([0-9a-f]{3,4}|[0-9a-f]{6}|[0-9a-f]{8})$/i.test(h)) h = "FFFFFF";
  if (h.length === 3 || h.length === 4) h = [...h].map((c) => c + c).join("");
  return { rgb: "#" + h.slice(0, 6).padEnd(6, "F"), a: h.length >= 8 ? parseInt(h.slice(6, 8), 16) : 255 };
}
function partsToHex(rgb, a) {
  const aa = a >= 255 ? "" : clamp(Math.round(a), 0, 255).toString(16).padStart(2, "0").toUpperCase();
  return rgb.toUpperCase() + aa;
}
const cssColor = (hex) => { const p = hexToParts(hex); return p.rgb + p.a.toString(16).padStart(2, "0"); };

/** Seletor de cor com alfa: amostra + hex + opacidade. */
function colorCtl(value, onChange) {
  const wrap = document.createElement("div");
  wrap.className = "color";
  const p = hexToParts(value);
  wrap.innerHTML = `<label class="swatch"><i style="background:${cssColor(value)}"></i><input type="color" value="${p.rgb}"></label>
    <input class="hex" value="${esc(value || "")}" spellcheck="false">
    <input class="alpha" type="range" min="0" max="255" value="${p.a}" title="Opacidade">`;
  const [picker, hex, alpha] = [wrap.querySelector("input[type=color]"), wrap.querySelector(".hex"), wrap.querySelector(".alpha")];
  const emit = (v, live) => { hex.value = v; wrap.querySelector("i").style.background = cssColor(v); onChange(v, live); };
  picker.oninput = () => emit(partsToHex(picker.value, +alpha.value), true);
  picker.onchange = () => emit(partsToHex(picker.value, +alpha.value), false);
  alpha.oninput = () => emit(partsToHex(picker.value, +alpha.value), true);
  alpha.onchange = () => emit(partsToHex(picker.value, +alpha.value), false);
  hex.onchange = () => { const v = hex.value.trim(); if (/^#?[0-9a-f]{3,8}$/i.test(v)) { const pp = hexToParts(v.startsWith("#") ? v : "#" + v); picker.value = pp.rgb; alpha.value = pp.a; emit(v.startsWith("#") ? v.toUpperCase() : "#" + v.toUpperCase(), false); } };
  return wrap;
}

class Form {
  constructor(root, target, { onLive, onCommit }) {
    this.root = root; this.t = target; this.onLive = onLive; this.onCommit = onCommit;
  }
  section(title) {
    const s = document.createElement("div");
    s.className = "section";
    if (title) s.innerHTML = `<h4>${title}</h4>`;
    this.root.appendChild(s);
    this.cur = s;
    return this;
  }
  _row(label, ctl, { scrub } = {}) {
    const r = document.createElement("div");
    r.className = "row";
    const lb = document.createElement("label");
    lb.textContent = label;
    r.appendChild(lb);
    const c = document.createElement("div");
    c.className = "ctl";
    c.appendChild(ctl);
    r.appendChild(c);
    (this.grid || this.cur).appendChild(r);
    if (scrub) { lb.classList.add("scrub"); scrub(lb); }
    return r;
  }
  set(key, v, live) {
    beginChange();
    this.t[key] = v;
    S.docVersion++;
    if (live) { this.onLive?.(); return; }
    commit();
    if (this.onCommit) this.onCommit(); else refreshPanels();
  }
  grid2(fn) {
    const g = document.createElement("div");
    g.className = "grid2";
    this.cur.appendChild(g);
    this.grid = g; fn(); this.grid = null;
    return this;
  }
  num(label, key, { step = 1, min = -9999, max = 9999, def = 0 } = {}) {
    const i = document.createElement("input");
    i.type = "number"; i.step = step; i.value = round(+(this.t[key] ?? def), 4); i.dataset.key = key;
    i.oninput = () => i.value !== "" && this.set(key, clamp(+i.value, min, max), true);
    i.onchange = () => i.value !== "" && this.set(key, clamp(+i.value, min, max), false);
    this._row(label, i, { scrub: (lb) => {
      lb.onpointerdown = (e) => {
        const x0 = e.clientX, v0 = +(this.t[key] ?? def);
        lb.setPointerCapture(e.pointerId);
        lb.onpointermove = (m) => { const v = clamp(round(v0 + (m.clientX - x0) * step * (m.shiftKey ? 10 : 1), 3), min, max); i.value = v; this.set(key, v, true); };
        lb.onpointerup = () => { lb.onpointermove = null; this.set(key, +i.value, false); };
      };
    }});
    return this;
  }
  range(label, key, { min = 0, max = 1, step = 0.01, def = 0 } = {}) {
    const w = document.createElement("div");
    w.style.cssText = "display:flex;gap:8px;align-items:center;width:100%";
    const i = document.createElement("input");
    i.type = "range"; i.min = min; i.max = max; i.step = step; i.value = this.t[key] ?? def;
    const v = document.createElement("span"); v.className = "range-val"; v.textContent = round(+i.value, 2);
    i.oninput = () => { v.textContent = i.value; this.set(key, +i.value, true); };
    i.onchange = () => this.set(key, +i.value, false);
    w.append(i, v);
    this._row(label, w);
    return this;
  }
  text(label, key, { placeholder = "" } = {}) {
    const i = document.createElement("input");
    i.value = this.t[key] ?? ""; i.placeholder = placeholder;
    i.oninput = () => this.set(key, i.value, true);
    i.onchange = () => this.set(key, i.value, false);
    this._row(label, i);
    return this;
  }
  optText(label, key, { placeholder = "" } = {}) {
    const i = document.createElement("input");
    i.value = this.t[key] ?? ""; i.placeholder = placeholder;
    const apply = (live) => { if (i.value.trim() === "") { beginChange(); delete this.t[key]; S.docVersion++; if (!live) commit(); } else this.set(key, i.value, live); };
    i.oninput = () => apply(true); i.onchange = () => apply(false);
    this._row(label, i);
    return this;
  }
  /** Número opcional: vazio remove o campo (ex.: mín/máx automáticos). */
  optNum(label, key, { placeholder = "auto" } = {}) {
    const i = document.createElement("input");
    i.type = "number"; i.step = "any"; i.placeholder = placeholder;
    i.value = this.t[key] ?? "";
    const apply = (live) => {
      if (i.value.trim() === "") { beginChange(); delete this.t[key]; S.docVersion++; if (!live) commit(); }
      else if (!isNaN(+i.value)) this.set(key, +i.value, live);
    };
    i.oninput = () => apply(true); i.onchange = () => apply(false);
    this._row(label, i);
    return this;
  }
  select(label, key, options, { def } = {}) {
    const s = document.createElement("select");
    for (const o of options) { const [v, t] = Array.isArray(o) ? o : [o, o]; s.add(new Option(t, v)); }
    s.value = this.t[key] ?? def ?? options[0];
    // Valor vazio remove o campo (mantém o TOML limpo).
    s.onchange = () => { if (s.value === "") change(() => delete this.t[key]); else this.set(key, s.value, false); };
    this._row(label, s);
    return this;
  }
  seg(label, key, options, def) {
    const cur = () => (this.t[key] == null || this.t[key] === "" ? def : this.t[key]);
    const render = () => seg(options, cur(), (v) => { this.set(key, v, false); row.querySelector(".ctl").replaceChildren(render()); });
    const row = this._row(label, render());
    return this;
  }
  check(label, key, def = false) {
    return this.seg(label, key, [[true, "Sim"], [false, "Não"]], def);
  }
  color(label, key, def = "#FFFFFF") {
    this._row(label, colorCtl(this.t[key] ?? def, (v, live) => this.set(key, v, live)));
    return this;
  }
  optColor(label, key) {
    const has = this.t[key] != null;
    const box = document.createElement("div");
    box.style.cssText = "display:flex;gap:6px;align-items:center;width:100%";
    if (has) {
      box.appendChild(colorCtl(this.t[key], (v, live) => this.set(key, v, live)));
      const x = document.createElement("button"); x.className = "x-btn"; x.textContent = "✕"; x.title = "Remover";
      x.onclick = () => { change(() => delete this.t[key]); };
      box.appendChild(x);
    } else {
      const b = document.createElement("button"); b.className = "link-btn"; b.textContent = "+ adicionar";
      b.onclick = () => change(() => (this.t[key] = "#000000AA"));
      box.appendChild(b);
    }
    this._row(label, box);
    return this;
  }
  gradient(label, key, { allowEmpty = false } = {}) {
    const list = this.t[key] || (this.t[key] = []);
    const f = document.createElement("div");
    f.className = "field";
    f.innerHTML = `<label>${label}</label><div class="grad-preview"></div><div class="stops"></div>`;
    const pv = f.querySelector(".grad-preview");
    const paint = () => (pv.style.background = list.length > 1 ? `linear-gradient(90deg, ${list.map(cssColor).join(",")})` : list.length ? cssColor(list[0]) : "transparent");
    const stops = f.querySelector(".stops");
    list.forEach((c, i) => {
      const r = document.createElement("div");
      r.className = "stop";
      r.appendChild(colorCtl(c, (v, live) => { list[i] = v; paint(); this.set(key, list, live); }));
      const x = document.createElement("button"); x.className = "x-btn"; x.textContent = "✕";
      x.onclick = () => { if (list.length > (allowEmpty ? 0 : 1)) change(() => list.splice(i, 1)); };
      r.appendChild(x);
      stops.appendChild(r);
    });
    const add = document.createElement("button"); add.className = "link-btn"; add.textContent = "+ cor";
    add.onclick = () => change(() => list.push(list[list.length - 1] || "#FFFFFF"));
    stops.appendChild(add);
    paint();
    this.cur.appendChild(f);
    return this;
  }
  /** Métrica ou expressão (ex.: `gpu_usage`, `ram_used_gb / ram_total_gb * 100`). */
  metric(label, key) {
    const i = document.createElement("input");
    i.setAttribute("list", "metricList");
    i.spellcheck = false;
    i.style.fontFamily = "var(--mono)";
    i.value = this.t[key] ?? "";
    i.placeholder = "cpu_usage ou expressão";
    i.title = "Nome de métrica ou expressão: + - * / % < > == && || e funções min, max, clamp, round, abs, sin, cos, if(c,a,b). Variáveis extras: t, hour, minute, second, day, month, weekday_num.";
    i.oninput = () => this.set(key, i.value, true);
    i.onchange = () => this.set(key, i.value, false);
    this._row(label, i);
    return this;
  }
  /** Lista de regras {when, color}. */
  rules(label, key) {
    const list = this.t[key] || (this.t[key] = []);
    const f = document.createElement("div");
    f.className = "field";
    f.innerHTML = `<label>${label}</label>`;
    list.forEach((r, i) => {
      const row = document.createElement("div");
      row.className = "stop";
      const w = document.createElement("input");
      w.value = r.when; w.placeholder = "cpu_temp > 80"; w.style.cssText = "flex:1;font-family:var(--mono);min-width:0";
      w.oninput = () => { r.when = w.value; this.set(key, list, true); };
      w.onchange = () => { r.when = w.value; this.set(key, list, false); };
      row.appendChild(w);
      const cc = colorCtl(r.color, (v, live) => { r.color = v; this.set(key, list, live); });
      cc.style.width = "130px"; cc.querySelector(".alpha").remove();
      row.appendChild(cc);
      const x = document.createElement("button"); x.className = "x-btn"; x.textContent = "✕";
      x.onclick = () => change(() => list.splice(i, 1));
      row.appendChild(x);
      f.appendChild(row);
    });
    const add = document.createElement("button"); add.className = "link-btn"; add.textContent = "+ regra";
    add.onclick = () => change(() => list.push({ when: "cpu_temp > 80", color: "#FF4040" }));
    f.appendChild(add);
    this.cur.appendChild(f);
    return this;
  }
  font(label, key) {
    const i = document.createElement("input");
    i.setAttribute("list", "fontList");
    i.value = this.t[key] ?? ""; i.placeholder = "segoeui.ttf";
    i.onchange = () => this.set(key, i.value, false);
    this._row(label, i);
    return this;
  }
  template(label, key) {
    const f = document.createElement("div");
    f.className = "field";
    f.innerHTML = `<label>${label}</label>`;
    const ta = document.createElement("textarea");
    ta.value = this.t[key] ?? "";
    ta.rows = 2;
    ta.oninput = () => this.set(key, ta.value, true);
    ta.onchange = () => this.set(key, ta.value, false);
    f.appendChild(ta);
    const chips = document.createElement("div");
    chips.className = "chips";
    const all = ["{time:%H:%M:%S}", "{time:%H:%M}", "{date:%d/%m/%Y}", "{weekday}", "{cpu_temp*1.8+32:.0}", "{ram_used_gb/ram_total_gb*100:.0}", "{max(cpu_temp, gpu_temp)}",
      ...(S.info?.numeric_keys || []).map((k) => `{${k}:.0}`), ...(S.info?.text_keys || []).map((k) => `{${k}}`)];
    for (const p of all) {
      const c = document.createElement("span");
      c.className = "chip"; c.textContent = p;
      c.onclick = () => {
        const a = ta.selectionStart ?? ta.value.length, b = ta.selectionEnd ?? a;
        ta.value = ta.value.slice(0, a) + p + ta.value.slice(b);
        ta.focus(); ta.selectionStart = ta.selectionEnd = a + p.length;
        this.set(key, ta.value, false);
      };
      chips.appendChild(c);
    }
    f.appendChild(chips);
    this.cur.appendChild(f);
    return this;
  }
  note(html) { const p = document.createElement("p"); p.className = "note"; p.innerHTML = html; this.cur.appendChild(p); return this; }
  button(text, fn, cls = "btn small") { const b = document.createElement("button"); b.className = cls; b.textContent = text; b.onclick = fn; this.cur.appendChild(b); return this; }
}

function renderProps() {
  const root = $("#tab-props");
  root.innerHTML = "";
  const l = S.doc?.layers[S.sel];
  if (!l) {
    root.innerHTML = `<div class="empty">Selecione uma camada no canvas ou na lista.<br><br>
      <b>Atalhos</b><br>Setas move · Shift+setas 10px<br>Ctrl+D duplica · Del remove<br>[ ] ordem · Ctrl+Z/Y desfaz</div>`;
    return;
  }
  const f = new Form(root, l, {});
  const geo = () => f.grid2(() => { f.num("X", "x"); f.num("Y", "y"); f.num("L", "width", { min: 1 }); f.num("A", "height", { min: 1 }); });

  switch (l.type) {
    case "text":
      f.section("Conteúdo").template("Texto (placeholders)", "text");
      f.section("Tipografia").font("Fonte", "font").num("Tamanho", "size", { min: 4, max: 300, step: 0.5 })
        .num("Espaçamento", "letter_spacing", { step: 0.1 }).color("Cor", "color_hex").optColor("Sombra", "shadow")
        .range("Opacidade", "opacity", { def: 1 }).check("Maiúsculas", "uppercase", false);
      f.section("Efeitos de texto").optColor("Contorno", "outline").num("Esp. contorno", "outline_width", { min: 0, max: 6, step: 0.5, def: 1 })
        .gradient("Degradê vertical (opcional)", "gradient", { allowEmpty: true });
      f.section("Posição").grid2(() => { f.num("X", "x"); f.num("Y", "y"); })
        .seg("Alinhar", "align", [["left", "Esq."], ["center", "Centro"], ["right", "Dir."]], "left")
        .seg("Âncora Y", "valign", [["top", "Topo"], ["middle", "Meio"], ["bottom", "Base"]], "top")
        .optNum("Larg. máx.", "max_width", { placeholder: "sem limite" });
      break;
    case "radial_gauge":
      f.section("Dados").metric("Métrica", "metric").grid2(() => { f.num("Mín", "min"); f.num("Máx", "max"); }).check("Suavizar", "smooth", true);
      f.section("Geometria").grid2(() => { f.num("X", "center_x"); f.num("Y", "center_y"); })
        .num("Raio", "radius", { min: 2 }).num("Espessura", "stroke_width", { min: 1, step: 0.5 })
        .num("Ângulo ini.", "start_angle").num("Ângulo fim", "end_angle").check("Pontas red.", "rounded_caps", true);
      f.section("Cores").gradient("Degradê", "color_gradient")
        .seg("Modo", "color_mode", [["arc", "Ao longo"], ["value", "Pelo valor"]], "arc").color("Trilho", "track_color", "#FFFFFF22");
      break;
    case "bar":
      f.section("Dados").metric("Métrica", "metric").grid2(() => { f.num("Mín", "min"); f.num("Máx", "max"); }).check("Suavizar", "smooth", true);
      f.section("Geometria"); geo();
      f.seg("Direção", "orientation", [["horizontal", "Horizontal"], ["vertical", "Vertical"]], "horizontal").num("Cantos", "radius", { min: 0 });
      f.section("Cores").gradient("Degradê", "color_gradient").color("Trilho", "track_color", "#FFFFFF22");
      break;
    case "sparkline":
      f.section("Dados").select("Métrica", "metric", S.info?.history_keys || []).num("Segundos", "seconds", { min: 2, max: 120 })
        .optNum("Mín", "min").optNum("Máx", "max");
      f.section("Geometria"); geo();
      f.section("Estilo").color("Cor", "color_hex").num("Espessura", "stroke_width", { min: 0.5, step: 0.1 }).range("Preench.", "fill_opacity", { def: 0.25 });
      break;
    case "image":
      f.section("Imagem").text("Arquivo", "path").grid2(() => { f.num("X", "x"); f.num("Y", "y"); f.num("L", "width", { min: 1 }); f.num("A", "height", { min: 1 }); })
        .range("Opacidade", "opacity", { def: 1 });
      break;
    case "cores":
      f.section("Grade").num("Colunas", "columns", { min: 0, max: 64, def: 0 }).num("Espaço", "gap", { min: 0, max: 20, step: 0.5, def: 2 })
        .seg("Direção", "orientation", [["vertical", "Vertical"], ["horizontal", "Horizontal"]], "vertical").num("Cantos", "radius", { min: 0 })
        .note("0 colunas = todos os núcleos numa linha.");
      f.section("Geometria"); geo();
      f.section("Cores").gradient("Degradê", "color_gradient").color("Trilho", "track_color", "#FFFFFF22");
      break;
    case "rect":
      f.section("Geometria"); geo();
      f.num("Cantos", "radius", { min: 0 });
      f.section("Aparência").color("Cor", "color_hex");
      break;
    case "shape":
      f.section("Forma").select("Tipo", "kind", [["rect", "Retângulo"], ["ellipse", "Elipse"], ["regular", "Polígono regular"], ["star", "Estrela"], ["polygon", "Forma livre"], ["line", "Linha / caminho"]]);
      if (l.kind === "regular" || l.kind === "star") f.num("Lados/pontas", "sides", { min: 2, max: 64 });
      if (l.kind === "star") f.range("Raio interno", "inner_ratio", { min: 0.05, max: 1, def: 0.5 });
      if (l.kind === "rect") f.num("Cantos", "corner_radius", { min: 0 });
      if (l.kind === "polygon" || l.kind === "line") f.note("Arraste os pontos ciano no canvas. <b>Duplo clique</b> adiciona ponto, <b>Alt+clique</b> remove.");
      f.section("Geometria"); geo();
      f.num("Rotação", "rotation", { step: 1 }).num("Repetir (radial)", "repeat", { min: 1, max: 64, def: 1 });
      if ((l.paths || []).length) f.note(`${l.paths.length} traço(s) extra(s) acompanham a forma e a repetição.`);
      if (l.kind !== "line") {
        f.section("Preenchimento").color("Cor sólida", "fill", "#FFFFFF").gradient("Degradê (opcional)", "fill_gradient", { allowEmpty: true })
          .seg("Degradê", "gradient_type", [["linear", "Linear"], ["radial", "Radial"]], "linear").num("Ângulo", "gradient_angle");
      }
      f.section("Contorno").optColor("Cor", "stroke").num("Espessura", "stroke_width", { min: 0, step: 0.5 }).range("Opacidade", "opacity", { def: 1 });
      if (l.kind !== "line") {
        f.section("Preencher por métrica");
        const has = !!l.metric;
        f.seg("Ativo", "_metricOn", [[true, "Sim"], [false, "Não"]], has);
        if (has) f.metric("Métrica", "metric").grid2(() => { f.num("Mín", "min"); f.num("Máx", "max"); }).num("Direção", "fill_direction").note("270° = de baixo para cima, 0° = da esquerda.");
      }
      break;
  }
  // ── Comportamentos comuns a todas as camadas ──
  f.section("Comportamento")
    .optText("Visível se", "visible_if", { placeholder: "sempre  (ex.: media_playing == 1)" })
    .range("Opacidade", "layer_opacity", { min: 0, max: 1, step: 0.01, def: 1 })
    .select("Animação", "animation", [["", "Nenhuma"], ["pulse", "Pulsar"], ["blink", "Piscar"], ["spin", "Girar (formas)"], ["marquee", "Letreiro (texto)"], ["fade_in", "Surgir"], ["slide_up", "Deslizar p/ cima"]], { def: "" })
    .num("Velocidade", "animation_speed", { min: 0.05, max: 20, step: 0.05, def: 1 })
    .optColor("Brilho", "glow").num("Raio brilho", "glow_radius", { min: 0, max: 24, step: 0.5, def: 6 })
    .rules("Cor condicional (1ª regra verdadeira vence)", "color_rules")
    .note("Expressões aceitam métricas, <code>t</code> (segundos desde o carregamento), <code>hour</code>, <code>minute</code>… <a href='#' id='lnkReplay'>Reiniciar animações</a>");
  root.querySelector("#lnkReplay").onclick = (e) => { e.preventDefault(); invoke("restart_animations"); };
  f.section().button("Duplicar", duplicate).button("Remover", () => removeLayer(S.sel), "btn small danger");
}

// Liga/desliga métrica de shape via campo virtual
function normalizeVirtual(l) {
  if (l && "_metricOn" in l) {
    if (l._metricOn && !l.metric) l.metric = "cpu_usage";
    if (!l._metricOn) delete l.metric;
    delete l._metricOn;
  }
}

function updateGeometryInputs() {
  // Atualiza só os campos numéricos visíveis durante o arraste (barato).
  const l = S.doc.layers[S.sel];
  if (!l) return;
  for (const i of document.querySelectorAll("#tab-props input[type=number]")) {
    const k = i.dataset.key;
    if (k && document.activeElement !== i && l[k] != null) i.value = l[k];
  }
  const [x, y, w, h] = boxOf(S.sel);
  $("#stSel").textContent = `${layerName(l)} — ${Math.round(x)}, ${Math.round(y)} · ${Math.round(w)}×${Math.round(h)}`;
}

function renderBackground() {
  const root = $("#tab-bg");
  root.innerHTML = "";
  if (!S.doc) return;
  const bg = S.doc.background;
  const f = new Form(root, bg, { onCommit: () => renderBackground() });
  f.section("Fundo").seg("Tipo", "type", [["solid", "Cor"], ["static", "Imagem"], ["gif", "Animação"], ["video", "Vídeo"]], "auto");
  if (bg.type === "solid") f.color("Cor", "color", "#000000");
  else {
    f.text("Arquivo", "path", { placeholder: "bg.png / bg.gif / bg.mp4" });
    f.button("Importar arquivo…", () => importBackground());
    f.seg("Ajuste", "fit", [["cover", "Preencher"], ["contain", "Conter"], ["stretch", "Esticar"]], "cover");
  }
  f.range("Escurecer", "dim", { min: 0, max: 0.9, step: 0.01, def: 0 });
  const m = new Form(root, S.doc.meta, { onCommit: () => {} });
  m.section("Tema").text("Nome", "name").text("Autor", "author").num("FPS máx.", "fps", { min: 1, max: 30, def: 24 })
    .font("Fonte padrão", "font");
  renderPalette(root);
  m.section().note("Formatos: PNG/JPG, GIF/WebP/APNG animados, MP4/H.264, HEVC, WMV. Vídeos são decodificados pelo Windows (Media Foundation).");
}

/** Editor da paleta do tema: cores nomeadas usadas como $nome em qualquer campo. */
function renderPalette(root) {
  S.doc.palette ??= {};
  const pal = S.doc.palette;
  const sec = document.createElement("div");
  sec.className = "section";
  sec.innerHTML = "<h4>Paleta</h4>";
  for (const [name, val] of Object.entries(pal)) {
    const row = document.createElement("div");
    row.className = "stop";
    const n = document.createElement("input");
    n.value = name; n.style.cssText = "width:96px;font-family:var(--mono)";
    n.onchange = () => change(() => { const v = pal[name]; delete pal[name]; pal[n.value.trim().replace(/^\$/, "") || name] = v; });
    row.appendChild(n);
    row.appendChild(colorCtl(val, (v, live) => { beginChange(); pal[name] = v; S.docVersion++; if (!live) commit(); }));
    const x = document.createElement("button"); x.className = "x-btn"; x.textContent = "✕";
    x.onclick = () => change(() => delete pal[name]);
    row.appendChild(x);
    sec.appendChild(row);
  }
  const add = document.createElement("button"); add.className = "link-btn"; add.textContent = "+ cor nomeada";
  add.onclick = () => { let i = 1; while (pal[`cor${i}`]) i++; change(() => (pal[`cor${i}`] = "#8B6CFF")); renderBackground(); };
  sec.appendChild(add);
  const p = document.createElement("p"); p.className = "note";
  p.innerHTML = "Use <code>$nome</code> em qualquer campo de cor (ex.: <code>$accent</code>). Mudar a paleta recolore o tema inteiro.";
  sec.appendChild(p);
  root.appendChild(sec);
}

async function renderDevice() {
  const root = $("#tab-device");
  root.innerHTML = "";
  if (!S.config) S.config = await invoke("get_config");
  const c = S.config;
  const save = debounce(async () => { try { await invoke("set_config", { cfg: c }); } catch (e) { toast(e, true); } }, 250);
  const f = new Form(root, c, { onLive: save, onCommit: () => { save(); renderDevice(); } });
  f.onLive = save;
  f.section("Painel").range("Brilho", "brightness", { min: 0, max: 100, step: 1 })
    .seg("Rotação", "rotation", [[0, "0°"], [90, "90°"], [180, "180°"], [270, "270°"]], 0)
    .check("Inverter V", "flip_vertical").check("Espelhar H", "flip_horizontal").check("Silencioso", "silent")
    .range("FPS máx.", "max_fps", { min: 1, max: 30, step: 1, def: 24 })
    .note("Padrão 24. Fundos animados custam CPU proporcional ao FPS (driver USB do Windows): ~0,30% a 15 fps, ~0,43% a 30 fps. Temas estáticos quase não são afetados.");
  f.section("Tela de inicialização").check("Exibir", "splash", true)
    .num("Duração (ms)", "splash_ms", { min: 300, max: 15000, step: 100, def: 2600 })
    .note("Edite o visual escolhendo <b>⚡ Tela de inicialização</b> na lista de temas.");
  f.section("Telemetria").seg("GPU", "gpu_backend", [["auto", "Auto (leve)"], ["nvml", "NVML"], ["pdh", "PDH"]], "auto")
    .note("NVML traz watts e fan % reais da NVIDIA, mas usa ~19 MB a mais de RAM. Requer reiniciar o serviço.")
    .text("Disco", "disk_mount").num("Cache anim. MB", "anim_cache_mb", { min: 1, max: 512 });
  f.section("Clima (Open-Meteo)");
  const w = new Form(root, c, { onLive: save, onCommit: save });
  w.cur = f.cur;
  w.optNum("Latitude", "weather_lat", { placeholder: "-23.55" }).optNum("Longitude", "weather_lon", { placeholder: "-46.63" })
    .note("Deixe vazio para desativar. Requer reiniciar o serviço.");
  f.section().note("As mudanças são gravadas no <b>config.toml</b> e aplicadas pelo serviço em ~1 s.");
}

function renderData() {
  const root = $("#tab-data");
  const rows = Object.entries(S.telemetry).map(([k, v]) => `<tr><td title="Clique para copiar o placeholder" data-k="${k}">${k}</td><td>${v == null ? "<span style='color:var(--faint)'>—</span>" : esc(v)}</td></tr>`).join("");
  root.innerHTML = `<div class="section"><h4>Telemetria ao vivo</h4><table class="metrics">${rows}</table>
    <p class="note">Clique em um nome para copiar <code>{nome}</code>. Valores “—” precisam do LibreHardwareMonitor ou do backend NVML.</p></div>`;
  root.querySelectorAll("td[data-k]").forEach((td) => td.onclick = () => { navigator.clipboard.writeText(`{${td.dataset.k}}`); toast(`{${td.dataset.k}} copiado`); });
}

function refreshPanels() {
  normalizeVirtual(S.doc?.layers[S.sel]);
  renderLayers();
  if (!S.drag) {
    const active = document.activeElement;
    const typingInProps = active && $("#tab-props").contains(active) && ["INPUT", "TEXTAREA"].includes(active.tagName);
    if (!typingInProps) renderProps();
  }
  const l = S.doc?.layers[S.sel];
  $("#stSel").textContent = l ? layerName(l) : "Nada selecionado";
  drawOverlay();
}

// ───────────────────────── Ações ─────────────────────────
function debounce(fn, ms) { let t; return (...a) => { clearTimeout(t); t = setTimeout(() => fn(...a), ms); }; }

async function pickFile(filters) {
  const r = await invoke("plugin:dialog|open", { options: { multiple: false, directory: false, filters } });
  return typeof r === "string" ? r : r?.path ?? null;
}

async function addImage() {
  const p = await pickFile([{ name: "Imagens", extensions: ["png", "jpg", "jpeg", "webp", "gif"] }]);
  if (!p) return;
  try {
    const name = await invoke("import_asset", { path: p });
    addLayers([{ type: "image", path: name, x: 10, y: 10, opacity: 1 }]);
  } catch (e) { toast(e, true); }
}

async function importBackground() {
  const p = await pickFile([{ name: "Mídia", extensions: ["png", "jpg", "jpeg", "gif", "webp", "apng", "mp4", "m4v", "mov", "wmv", "avi", "mkv"] }]);
  if (!p) return;
  try {
    const name = await invoke("import_asset", { path: p });
    const ext = name.split(".").pop().toLowerCase();
    const type = ["mp4", "m4v", "mov", "wmv", "avi", "mkv"].includes(ext) ? "video" : ["gif", "webp", "apng"].includes(ext) ? "gif" : "static";
    change(() => Object.assign(S.doc.background, { type, path: name }));
    renderBackground();
  } catch (e) { toast(e, true); }
}

async function save() {
  if (!S.doc) return;
  try {
    await invoke("save_theme", { doc: S.doc });
    markDirty(false);
    toast("Tema salvo — o display atualiza sozinho se este tema estiver ativo");
  } catch (e) { toast(e, true); }
}

async function loadThemes(selectId) {
  S.themes = await invoke("list_themes");
  const sel = $("#themeSelect");
  sel.innerHTML = "";
  for (const t of S.themes) sel.add(new Option(`${t.name}${t.active ? "  ●" : ""}`, t.id));
  $("#metricList").innerHTML = (S.info?.numeric_keys || []).concat(["t", "hour", "minute", "second"]).map((k) => `<option value="${k}">`).join("");
  const id = selectId || S.themeId || S.themes.find((t) => t.active)?.id || S.themes[0]?.id;
  if (id) { sel.value = id; await openTheme(id); }
}

async function openTheme(id) {
  if (S.dirty && !confirm("Há alterações não salvas. Descartar?")) { $("#themeSelect").value = S.themeId; return; }
  try {
    const doc = await invoke("open_theme", { id });
    doc.background.type ??= doc.background.kind;
    S.doc = doc; S.themeId = id; S.sel = -1; S.undo = []; S.redo = [];
    $("#btnActivate").disabled = id === "@splash";
    if (id === "@splash") toast("Editando a tela de inicialização — exibida ao ligar o serviço");
    // Reenvia imediatamente: o motor passa a usar z = índice da camada,
    // alinhando as caixas de seleção com a lista.
    S.docVersion++; S.sentVersion = -1;
    markDirty(false);
    refreshPanels(); renderBackground();
  } catch (e) { toast(e, true); }
}

function switchTab(name) {
  document.querySelectorAll("#tabs button").forEach((b) => b.classList.toggle("active", b.dataset.tab === name));
  document.querySelectorAll(".tab-body").forEach((b) => b.classList.toggle("hidden", b.id !== `tab-${name}`));
  if (name === "device") renderDevice();
  if (name === "data") renderData();
  if (name === "bg") renderBackground();
}

function setZoom(z) {
  S.zoom = clamp(z, 1, 6);
  const scr = $("#screen");
  scr.style.width = `${W * S.zoom}px`; scr.style.height = `${H * S.zoom}px`;
  $("#zoomLabel").textContent = `${Math.round(S.zoom * 100)}%`;
  drawOverlay();
}
function fitZoom() {
  const r = $("#stage").getBoundingClientRect();
  setZoom(Math.max(1, Math.floor(Math.min((r.width - 100) / W, (r.height - 100) / H) * 2) / 2));
}

async function pollDaemon() {
  try {
    const on = await invoke("daemon_status");
    const p = $("#daemonPill");
    p.classList.toggle("on", on);
    p.querySelector("span").textContent = on ? "Display ativo" : "Serviço parado";
    p.title = on ? "kalkan-rs em execução" : "Clique para iniciar o kalkan-rs";
  } catch {}
}

async function pollTelemetry() {
  try {
    S.telemetry = await invoke("telemetry");
    if (!$("#tab-data").classList.contains("hidden")) renderData();
  } catch {}
}

// ───────────────────────── Inicialização ─────────────────────────
async function init() {
  // A camada de fundo usa "type" no TOML; o serde do Rust usa o mesmo nome.
  S.info = await invoke("app_info");
  $("#fontList").innerHTML = S.info.fonts.map((f) => `<option value="${esc(f)}">`).join("");

  const pal = $("#palette");
  for (const p of PALETTE) {
    const b = document.createElement("button");
    b.className = "tool"; b.innerHTML = `${p.icon}<span>${p.label}</span>`;
    b.onclick = () => (p.action ? p.action() : addLayers([p.make()]));
    pal.appendChild(b);
  }
  const pre = $("#presets");
  for (const p of PRESETS) {
    const b = document.createElement("button");
    b.className = "preset"; b.innerHTML = `<b>${p.name}</b>${p.desc}`;
    b.onclick = () => addLayers(p.layers());
    pre.appendChild(b);
  }

  $("#themeSelect").onchange = (e) => openTheme(e.target.value);
  $("#btnNewTheme").onclick = async () => {
    const name = await ask("Nome do novo tema");
    if (!name) return;
    try { const id = await invoke("create_theme", { name, from: null }); markDirty(false); await loadThemes(id); toast("Tema criado"); } catch (e) { toast(e, true); }
  };
  $("#btnDupTheme").onclick = async () => {
    if (!S.themeId) return;
    const name = await ask("Nome da cópia", `${S.doc.meta.name || S.themeId} (cópia)`);
    if (!name) return;
    try { if (S.dirty) await save(); const id = await invoke("create_theme", { name, from: S.themeId }); await loadThemes(id); toast("Tema duplicado"); } catch (e) { toast(e, true); }
  };
  $("#btnSave").onclick = save;
  $("#btnActivate").onclick = async () => {
    try {
      if (S.dirty) await save();
      await invoke("activate_theme", { id: S.themeId });
      S.config = null;
      await loadThemes(S.themeId);
      toast("Tema enviado ao display");
    } catch (e) { toast(e, true); }
  };
  $("#daemonPill").onclick = async () => { try { await invoke("start_daemon"); setTimeout(pollDaemon, 800); } catch (e) { toast(e, true); } };
  $("#btnUndo").onclick = undo;
  $("#btnRedo").onclick = redo;
  $("#btnZoomIn").onclick = () => setZoom(S.zoom + 0.5);
  $("#btnZoomOut").onclick = () => setZoom(S.zoom - 0.5);
  const toggle = (id, key) => { const b = $(id); b.onclick = () => { S[key] = !S[key]; b.setAttribute("aria-pressed", S[key]); drawOverlay(); }; };
  toggle("#btnSnap", "snap"); toggle("#btnGrid", "grid");
  $("#stageWrap").addEventListener("wheel", (e) => { if (e.ctrlKey) { e.preventDefault(); setZoom(S.zoom + (e.deltaY < 0 ? 0.5 : -0.5)); } }, { passive: false });
  document.querySelectorAll("#tabs button").forEach((b) => (b.onclick = () => switchTab(b.dataset.tab)));
  window.addEventListener("beforeunload", (e) => { if (S.dirty) { e.preventDefault(); e.returnValue = ""; } });

  requestAnimationFrame(frame);
  setInterval(pollDaemon, 3000);
  setInterval(pollTelemetry, 1000);
  pollDaemon();
  fitZoom();
  window.addEventListener("resize", debounce(fitZoom, 150));
  await pollTelemetry();
  await loadThemes();
  markDirty(false);
}

init().catch((e) => toast(String(e), true));
