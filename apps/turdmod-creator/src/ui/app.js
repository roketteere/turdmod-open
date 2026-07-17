// turdmod-creator GUI — vanilla JS, no framework.
// Talks to the local backend at the same origin.
const API = {
  async get(path) {
    const r = await fetch(path);
    if (!r.ok) throw new Error(`${path}: ${r.status}`);
    return r.json();
  },
  async post(path, body) {
    const r = await fetch(path, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    const j = await r.json();
    if (!r.ok) throw new Error(j.error ?? r.statusText);
    return j;
  },
};

const state = {
  project: null,
  projectDir: null,
  templates: [],
  currentTemplate: null,
  paramValues: {},
  advanced: false,
};

// ── Bootstrap ────────────────────────────────────────────────────────
async function boot() {
  state.templates = (await API.get("/api/templates")).templates;
  await refreshProject();
  renderGallery();
  bindUi();
  toast("turdmod-creator GUI ready. Pick a template to start.", "ok");
}

async function refreshProject() {
  const r = await API.get("/api/project");
  state.project = r.project;
  state.projectDir = r.dir;
  renderProject();
}

function renderProject() {
  const el = id => document.getElementById(id);
  if (state.project) {
    el("project-name").textContent = state.project.name;
    el("project-author").textContent = `by ${state.project.author ?? "Anonymous"}`;
    el("project-dir").textContent = state.projectDir;
    el("widget-count").textContent = state.project.widgets.length;
    const ul = el("widget-list");
    ul.innerHTML = "";
    for (const w of state.project.widgets) {
      const li = document.createElement("li");
      const left = document.createElement("div");
      left.innerHTML = `<div class="name">${esc(w.name)}</div><div class="template">${esc(w.template)} v${esc(w.templateVersion ?? "")}</div>`;
      const btn = document.createElement("button");
      btn.textContent = "✕";
      btn.title = "delete widget";
      btn.addEventListener("click", () => deleteWidget(w.name));
      li.append(left, btn);
      ul.append(li);
    }
  } else {
    el("project-name").textContent = "— no project here —";
    el("project-author").textContent = "click +New project";
    el("project-dir").textContent = state.projectDir ?? "";
    el("widget-count").textContent = "0";
    el("widget-list").innerHTML = "";
  }
}

function renderGallery() {
  const g = document.getElementById("template-gallery");
  g.innerHTML = "";
  for (const t of state.templates) {
    const c = document.createElement("div");
    c.className = "tpl-card";
    c.innerHTML = `
      <span class="cat">${esc(t.category)}</span>
      <h3>${esc(t.name)}</h3>
      <p>${esc(t.description)}</p>
      <div class="meta">v${esc(t.version)} · ${t.parameters.length} param${t.parameters.length===1?"":"s"}</div>
    `;
    c.addEventListener("click", () => openTemplate(t));
    g.append(c);
  }
}

function openTemplate(tpl) {
  state.currentTemplate = tpl;
  state.paramValues = {};
  state._welcomeTabIdx = 0;
  for (const p of tpl.parameters) {
    if (p.default !== undefined) state.paramValues[p.name] = p.default;
  }
  document.getElementById("template-gallery").classList.add("hidden");
  document.getElementById("param-form").classList.remove("hidden");
  document.getElementById("page-title").textContent = `Configure: ${tpl.name}`;
  document.getElementById("page-sub").textContent = "Fill in the basics, see the preview, then add to your project.";
  document.getElementById("param-title").textContent = tpl.description;
  document.getElementById("param-desc").textContent = `Template ${tpl.name} v${tpl.version} (${tpl.category})`;
  document.getElementById("widget-name").value = "";
  renderParamFields();
  renderPreview();
}

function paramIsAdvanced(p) {
  // Heuristic: any param with a default and not in the "core" set
  // is advanced. For v1 we just treat params with an empty string
  // default as advanced (e.g. soundCue, openOnEvent).
  if (p.default === "" || p.name === "soundCue" || p.name === "openOnEvent") return true;
  return false;
}

function renderParamFields() {
  const el = document.getElementById("param-fields");
  el.innerHTML = "";
  const tpl = state.currentTemplate;
  const visible = tpl.parameters.filter(p => state.advanced || !paramIsAdvanced(p));
  for (const p of visible) {
    el.append(buildField(p));
  }
  // Advanced toggle
  const tog = document.createElement("div");
  tog.className = "advanced-toggle";
  const hidden = tpl.parameters.length - visible.length;
  tog.textContent = state.advanced
    ? "▼ Hide advanced options"
    : `▶ Show advanced (${hidden} more parameter${hidden===1?"":"s"})`;
  if (state.advanced || hidden > 0) {
    tog.addEventListener("click", () => { state.advanced = !state.advanced; renderParamFields(); });
    el.append(tog);
  }
}

function buildField(p) {
  const wrap = document.createElement("div");
  wrap.className = "param-field";
  const value = state.paramValues[p.name] ?? p.default ?? "";
  let inputHtml;
  if (p.type === "color") {
    inputHtml = `<input type="color" data-param="${p.name}" value="${esc(String(value))}">`;
  } else if (p.type === "bool") {
    inputHtml = `<select data-param="${p.name}">
      <option value="true" ${value===true||value==="true"?"selected":""}>true</option>
      <option value="false" ${value===false||value==="false"?"selected":""}>false</option>
    </select>`;
  } else if (p.type === "enum") {
    const opts = (p.enum ?? []).map(v => `<option ${v===value?"selected":""}>${esc(v)}</option>`).join("");
    inputHtml = `<select data-param="${p.name}">${opts}</select>`;
  } else if (p.type === "int") {
    const min = p.min !== undefined ? `min="${p.min}"` : "";
    const max = p.max !== undefined ? `max="${p.max}"` : "";
    inputHtml = `<input type="number" data-param="${p.name}" ${min} ${max} value="${esc(String(value))}">`;
  } else {
    inputHtml = `<input type="text" data-param="${p.name}" value="${esc(String(value))}">`;
  }
  wrap.innerHTML = `
    <label>${esc(p.name)}</label>
    <span class="help">${esc(p.description)}</span>
    ${inputHtml}
  `;
  const input = wrap.querySelector(`[data-param="${p.name}"]`);
  input.addEventListener("input", () => {
    let v = input.value;
    if (p.type === "int")  v = parseInt(v, 10);
    if (p.type === "bool") v = v === "true";
    state.paramValues[p.name] = v;
    renderPreview();
  });
  return wrap;
}

// ── Preview renderers ────────────────────────────────────────────────
function renderPreview() {
  const preview = document.getElementById("preview");
  preview.innerHTML = "";
  const tpl = state.currentTemplate;
  if (!tpl) return;
  const p = state.paramValues;
  switch (tpl.name) {
    case "notification":    return renderNotificationPreview(preview, p);
    case "healing-wheel":   return renderWheelPreview(preview, p);
    case "kit-picker":      return renderKitPickerPreview(preview, p);
    case "welcome-window":  return renderWelcomeWindowPreview(preview, p);
    case "blank":           return renderBlankPreview(preview, p);
    default: preview.textContent = "(no preview)";
  }
}

function renderNotificationPreview(host, p) {
  const banner = document.createElement("div");
  banner.style.cssText = `
    width: 80%; max-width: 480px;
    background: ${p.bgColor || "#1a1a1a"};
    color: ${p.textColor || "#FFD700"};
    font-size: ${Math.min(p.fontSize || 32, 36)}px;
    text-align: center;
    padding: 16px 24px;
    border-radius: 4px;
    border: 1px solid ${(p.textColor || "#FFD700") + "33"};
    box-shadow: 0 4px 16px rgba(0,0,0,0.4);
    font-weight: 600;
  `;
  banner.textContent = p.text || "(empty)";
  host.append(banner);
  const meta = document.createElement("div");
  meta.style.cssText = "margin-top: 12px; color: #888; font-size: 12px;";
  meta.textContent = `auto-dismiss after ${p.durationMs || 10000}ms`;
  host.append(meta);
}

function renderWheelPreview(host, p) {
  const radius = Math.min(p.wheelRadius || 220, 200);
  const segments = parseInt(p.segmentCount || 6, 10);
  const labels = String(p.segmentLabels || "").split(",").map(s => s.trim());
  const svgNs = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(svgNs, "svg");
  svg.setAttribute("width", radius * 2);
  svg.setAttribute("height", radius * 2);
  svg.setAttribute("viewBox", `0 0 ${radius * 2} ${radius * 2}`);
  for (let i = 0; i < segments; i++) {
    const a0 = (i / segments) * 2 * Math.PI - Math.PI / 2;
    const a1 = ((i + 1) / segments) * 2 * Math.PI - Math.PI / 2;
    const x0 = radius + radius * Math.cos(a0);
    const y0 = radius + radius * Math.sin(a0);
    const x1 = radius + radius * Math.cos(a1);
    const y1 = radius + radius * Math.sin(a1);
    const path = document.createElementNS(svgNs, "path");
    path.setAttribute("d", `M${radius},${radius} L${x0},${y0} A${radius},${radius} 0 0,1 ${x1},${y1} Z`);
    path.setAttribute("fill", i === 0 ? (p.primaryColor || "#FFD700") : (p.accentColor || "#444"));
    path.setAttribute("stroke", "#0c0c10");
    path.setAttribute("stroke-width", "2");
    svg.append(path);
    // Label
    const labelAngle = (a0 + a1) / 2;
    const lx = radius + (radius * 0.65) * Math.cos(labelAngle);
    const ly = radius + (radius * 0.65) * Math.sin(labelAngle);
    const text = document.createElementNS(svgNs, "text");
    text.setAttribute("x", lx);
    text.setAttribute("y", ly);
    text.setAttribute("text-anchor", "middle");
    text.setAttribute("dominant-baseline", "middle");
    text.setAttribute("fill", "#fff");
    text.setAttribute("font-size", "13");
    text.setAttribute("font-weight", "600");
    text.textContent = labels[i] ?? `Slot ${i+1}`;
    svg.append(text);
  }
  // Center dot
  const dot = document.createElementNS(svgNs, "circle");
  dot.setAttribute("cx", radius);
  dot.setAttribute("cy", radius);
  dot.setAttribute("r", radius * 0.15);
  dot.setAttribute("fill", "#0c0c10");
  dot.setAttribute("stroke", p.primaryColor || "#FFD700");
  dot.setAttribute("stroke-width", "3");
  svg.append(dot);
  host.append(svg);
  const meta = document.createElement("div");
  meta.style.cssText = "margin-top: 12px; color: #888; font-size: 12px;";
  meta.textContent = `hotkey: ${p.hotkey || "H"}  ·  ${segments} segments`;
  host.append(meta);
}

function renderKitPickerPreview(host, p) {
  const cols = parseInt(p.gridCols || 4, 10);
  const rows = parseInt(p.gridRows || 3, 10);
  const slotSize = Math.min(parseInt(p.slotSize || 96, 10), 70);
  const wrap = document.createElement("div");
  wrap.style.cssText = `
    background: ${p.bgColor || "#0c0c0c"};
    padding: 16px;
    border-radius: 6px;
    border: 1px solid rgba(255,255,255,0.08);
  `;
  const title = document.createElement("div");
  title.style.cssText = "color: #ccc; font-weight: 600; margin-bottom: 12px;";
  title.textContent = p.title || "Choose a kit";
  wrap.append(title);
  const grid = document.createElement("div");
  grid.style.cssText = `display: grid; grid-template-columns: repeat(${cols}, ${slotSize}px); gap: 6px;`;
  for (let i = 0; i < cols * rows; i++) {
    const slot = document.createElement("div");
    const selected = i === 0;
    slot.style.cssText = `
      width: ${slotSize}px; height: ${slotSize}px;
      background: ${selected ? (p.selectedColor || "#FFD700") + "22" : "rgba(255,255,255,0.04)"};
      border: 1px solid ${selected ? (p.selectedColor || "#FFD700") : "rgba(255,255,255,0.1)"};
      border-radius: 4px;
      display: flex; align-items: center; justify-content: center;
      font-size: 10px; color: rgba(255,255,255,0.4);
    `;
    slot.textContent = `slot ${i+1}`;
    grid.append(slot);
  }
  wrap.append(grid);
  host.append(wrap);
}

function renderWelcomeWindowPreview(host, p) {
  // Rust-style server welcome window.
  // Scale the preview proportionally to fit the preview pane.
  const PREVIEW_MAX_W = 480;
  const PREVIEW_MAX_H = 360;
  const targetW = Math.min(parseInt(p.width || 720, 10), 1920);
  const targetH = Math.min(parseInt(p.height || 520, 10), 1080);
  const scale = Math.min(PREVIEW_MAX_W / targetW, PREVIEW_MAX_H / targetH);
  const w = targetW * scale;
  const h = targetH * scale;
  const fontSize = Math.max(8, 14 * scale);
  const titleFontSize = Math.max(11, 22 * scale);
  const subtitleFontSize = Math.max(9, 13 * scale);
  const tabFontSize = Math.max(8, 12 * scale);
  const bodyFontSize = Math.max(8, 13 * scale);
  const buttonFontSize = Math.max(8, 12 * scale);

  let tabs = [];
  let buttons = [];
  try { tabs = JSON.parse(p.tabsJson || "[]"); } catch {}
  try { buttons = JSON.parse(p.buttonsJson || "[]"); } catch {}

  // Active tab — first one by default. Allow click to switch.
  if (state._welcomeTabIdx === undefined) state._welcomeTabIdx = 0;
  if (state._welcomeTabIdx >= tabs.length) state._welcomeTabIdx = 0;
  const activeIdx = state._welcomeTabIdx;
  const active = tabs[activeIdx] || { name: "(empty)", body: "" };

  const win = document.createElement("div");
  win.style.cssText = `
    width: ${w}px; height: ${h}px;
    background: ${p.bgColor || "#0a0a0a"};
    color: ${p.textColor || "#e0e0e0"};
    border: 1px solid ${(p.accentColor || "#f5c542") + "44"};
    border-radius: ${4 * scale}px;
    box-shadow: 0 ${8*scale}px ${24*scale}px rgba(0,0,0,0.6),
                0 0 ${20*scale}px ${(p.accentColor || "#f5c542") + "22"};
    display: flex; flex-direction: column;
    overflow: hidden;
    font-family: -apple-system, system-ui, sans-serif;
  `;

  // Header bar
  const header = document.createElement("div");
  header.style.cssText = `
    background: ${p.headerColor || "#1a1a1a"};
    border-bottom: 1px solid ${(p.accentColor || "#f5c542") + "33"};
    padding: ${10*scale}px ${16*scale}px;
    display: flex; align-items: center; justify-content: space-between;
  `;
  header.innerHTML = `
    <div>
      <div style="font-size: ${titleFontSize}px; font-weight: 700; color: ${p.accentColor || "#f5c542"};">${esc(p.title || "Server")}</div>
      <div style="font-size: ${subtitleFontSize}px; color: ${p.textColor || "#e0e0e0"}99; margin-top: ${2*scale}px;">${esc(p.subtitle || "")}</div>
    </div>
    ${p.dismissible === false ? "" : `<div style="font-size: ${fontSize}px; color: ${p.textColor || "#e0e0e0"}88; cursor: pointer; user-select: none;">✕</div>`}
  `;
  win.append(header);

  // Tab bar
  if (tabs.length > 0) {
    const tabBar = document.createElement("div");
    tabBar.style.cssText = `
      display: flex; background: ${(p.bgColor || "#0a0a0a")};
      border-bottom: 1px solid ${(p.accentColor || "#f5c542") + "22"};
    `;
    tabs.forEach((t, i) => {
      const tab = document.createElement("div");
      const isActive = i === activeIdx;
      tab.style.cssText = `
        flex: 0 0 auto;
        padding: ${8*scale}px ${16*scale}px;
        font-size: ${tabFontSize}px;
        font-weight: ${isActive ? 600 : 400};
        color: ${isActive ? (p.accentColor || "#f5c542") : (p.textColor || "#e0e0e0") + "99"};
        border-bottom: ${2*scale}px solid ${isActive ? (p.accentColor || "#f5c542") : "transparent"};
        cursor: pointer;
        user-select: none;
      `;
      tab.textContent = t.name || `Tab ${i+1}`;
      tab.addEventListener("click", () => {
        state._welcomeTabIdx = i;
        renderPreview();
      });
      tabBar.append(tab);
    });
    win.append(tabBar);
  }

  // Body
  const body = document.createElement("div");
  body.style.cssText = `
    flex: 1; overflow-y: auto;
    padding: ${14*scale}px ${18*scale}px;
    font-size: ${bodyFontSize}px;
    line-height: 1.55;
    white-space: pre-wrap;
    color: ${p.textColor || "#e0e0e0"};
  `;
  body.textContent = active.body || "";
  win.append(body);

  // Buttons footer
  if (buttons.length > 0) {
    const footer = document.createElement("div");
    footer.style.cssText = `
      display: flex; gap: ${8*scale}px; justify-content: flex-end;
      padding: ${10*scale}px ${16*scale}px;
      background: ${p.headerColor || "#1a1a1a"};
      border-top: 1px solid ${(p.accentColor || "#f5c542") + "22"};
    `;
    buttons.forEach(b => {
      const btn = document.createElement("button");
      const isPrimary = b.action === "close";
      btn.style.cssText = `
        font-size: ${buttonFontSize}px;
        padding: ${6*scale}px ${14*scale}px;
        border-radius: ${3*scale}px;
        background: ${isPrimary ? (p.accentColor || "#f5c542") : "transparent"};
        color: ${isPrimary ? "#000" : (p.accentColor || "#f5c542")};
        border: 1px solid ${p.accentColor || "#f5c542"};
        cursor: pointer;
        font-weight: 600;
      `;
      btn.textContent = b.label || "Button";
      footer.append(btn);
    });
    win.append(footer);
  }

  host.append(win);

  // Meta line
  const meta = document.createElement("div");
  meta.style.cssText = `margin-top: ${10*scale}px; color: #888; font-size: 11px; text-align: center;`;
  const trigger = p.showOnLogin ? `auto-shows on login +${p.showOnGroundDelayMs || 5000}ms` : "manual show only";
  const reopen = p.showAgainKey ? ` · re-open key: ${p.showAgainKey}` : "";
  meta.textContent = `${targetW}×${targetH}  ·  ${trigger}${reopen}`;
  host.append(meta);
}

function renderBlankPreview(host, p) {
  const [w, h] = String(p.panelSize || "800x600").split("x").map(s => parseInt(s, 10));
  const aspect = Math.min(360 / (w||800), 240 / (h||600));
  const panel = document.createElement("div");
  panel.style.cssText = `
    width: ${(w||800) * aspect}px; height: ${(h||600) * aspect}px;
    background: rgba(255,255,255,0.04);
    border: 1px dashed rgba(255,255,255,0.3);
    border-radius: 4px;
    display: flex; align-items: center; justify-content: center;
    color: rgba(255,255,255,0.4);
    font-size: 12px;
  `;
  panel.textContent = `Blank ${w||"?"}×${h||"?"} (${p.anchorX || "Center"} / ${p.anchorY || "Center"})`;
  host.append(panel);
}

// ── Actions ──────────────────────────────────────────────────────────
function bindUi() {
  document.getElementById("btn-back").addEventListener("click", () => {
    document.getElementById("template-gallery").classList.remove("hidden");
    document.getElementById("param-form").classList.add("hidden");
    document.getElementById("page-title").textContent = "Pick a template";
    document.getElementById("page-sub").textContent = "Or use AI to draft one for you in the sidebar.";
  });
  document.getElementById("btn-add").addEventListener("click", addWidget);
  document.getElementById("btn-init").addEventListener("click", initProjectFlow);
  document.getElementById("btn-doctor").addEventListener("click", showDoctor);
  document.getElementById("btn-config").addEventListener("click", showConfig);
  document.getElementById("btn-ai").addEventListener("click", askAi);
}

async function initProjectFlow() {
  const name = await prompt("New project name", "my-widgets", v =>
    /^[a-zA-Z][a-zA-Z0-9_-]*$/.test(v) ? null : "use letters, digits, _ or -; start with a letter");
  if (!name) return;
  const author = await prompt("Author display name", "Anonymous", null);
  if (author === null) return;
  try {
    const r = await API.post("/api/init", { name, author });
    toast(`project created: ${r.dir}`, "ok");
    state.projectDir = r.dir;
    state.project = r.project;
    renderProject();
  } catch (e) {
    toast(e.message, "error");
  }
}

async function addWidget() {
  if (!state.project) {
    toast("Create a project first (click +New project).", "error");
    return;
  }
  const name = document.getElementById("widget-name").value.trim();
  if (!/^[a-zA-Z][a-zA-Z0-9_-]*$/.test(name)) {
    toast("Widget name: letters, digits, _ or -; must start with letter", "error");
    return;
  }
  try {
    const r = await API.post("/api/widget", {
      dir: state.projectDir,
      template: state.currentTemplate.name,
      name,
      parameters: state.paramValues,
    });
    state.project = r.project;
    renderProject();
    toast(`added: ${name}`, "ok");
    document.getElementById("widget-name").value = "";
    document.getElementById("btn-back").click();
  } catch (e) {
    toast(e.message, "error");
  }
}

async function deleteWidget(name) {
  const ok = await confirm(`Delete widget "${name}"?`);
  if (!ok) return;
  try {
    const r = await API.post("/api/widget/delete", { dir: state.projectDir, name });
    state.project = r.project;
    renderProject();
    toast(`deleted: ${name}`, "ok");
  } catch (e) {
    toast(e.message, "error");
  }
}

async function askAi() {
  const prompt = document.getElementById("ai-prompt").value.trim();
  const provider = document.getElementById("ai-provider").value;
  const model = document.getElementById("ai-model").value.trim();
  if (!prompt) return;
  const out = document.getElementById("ai-output");
  out.textContent = "(thinking...)";
  try {
    const r = await API.post("/api/ai", { provider, model, prompt });
    out.textContent = r.result.text;
    if (r.result.estimatedUSD !== undefined) {
      out.textContent += `\n\n— est cost ~$${r.result.estimatedUSD.toFixed(4)} (${r.result.promptTokens ?? "?"} in / ${r.result.completionTokens ?? "?"} out)`;
    }
  } catch (e) {
    out.textContent = `ERROR: ${e.message}\n\nIf this is "API key env var ... is not set" — that's expected. Your key, your billing. Set it in your shell + restart the GUI.`;
  }
}

async function showDoctor() {
  const r = await API.get("/api/doctor");
  const items = r.checks.map(c =>
    `<li class="${c.ok ? "ok" : "fail"}">${esc(c.name)}<div class="detail">${esc(c.detail)}</div></li>`,
  ).join("");
  await infoModal("Doctor — setup health", `<ul class="checklist">${items}</ul>`);
}

async function showConfig() {
  const r = await API.get("/api/config");
  const c = r.config ?? {};
  const body = `
    <p class="muted small">Edits save to ~/.turdmod-creator/config.json. Used by future runs.</p>
    <label>UE 4.27 path<input id="cfg-ue" value="${esc(c.uePath ?? "")}"></label>
    <label>AI provider<select id="cfg-prov">
      <option value="">(unset)</option>
      <option value="openai" ${c.aiProvider==="openai"?"selected":""}>openai</option>
      <option value="anthropic" ${c.aiProvider==="anthropic"?"selected":""}>anthropic</option>
      <option value="deepseek" ${c.aiProvider==="deepseek"?"selected":""}>deepseek</option>
      <option value="ollama" ${c.aiProvider==="ollama"?"selected":""}>ollama</option>
      <option value="gemini" ${c.aiProvider==="gemini"?"selected":""}>gemini</option>
    </select></label>
    <label>AI model<input id="cfg-model" value="${esc(c.aiModel ?? "")}"></label>
    <label>Key env var name<input id="cfg-keyenv" value="${esc(c.keyEnvVar ?? "TURDMOD_AI_KEY")}"></label>
  `;
  const ok = await infoModal("Config", body, true);
  if (!ok) return;
  const newCfg = {
    uePath: document.getElementById("cfg-ue").value || undefined,
    aiProvider: document.getElementById("cfg-prov").value || undefined,
    aiModel: document.getElementById("cfg-model").value || undefined,
    keyEnvVar: document.getElementById("cfg-keyenv").value || undefined,
  };
  await API.post("/api/config", newCfg);
  toast("config saved", "ok");
}

// ── Tiny modal + toast helpers (no framework) ────────────────────────
function esc(s) {
  return String(s ?? "").replace(/[&<>"']/g, c => ({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","'":"&#39;"}[c]));
}

function toast(msg, kind = "") {
  const el = document.getElementById("toast");
  el.textContent = msg;
  el.className = "toast " + kind;
  setTimeout(() => el.classList.add("hidden"), 4000);
}

function infoModal(title, bodyHtml, hasCancel = false) {
  return new Promise(resolve => {
    const modal = document.getElementById("modal");
    document.getElementById("modal-title").textContent = title;
    document.getElementById("modal-body").innerHTML = bodyHtml;
    modal.classList.remove("hidden");
    document.getElementById("modal-cancel").style.display = hasCancel ? "" : "none";
    const cleanup = (val) => {
      modal.classList.add("hidden");
      document.getElementById("modal-ok").onclick = null;
      document.getElementById("modal-cancel").onclick = null;
      resolve(val);
    };
    document.getElementById("modal-ok").onclick = () => cleanup(true);
    document.getElementById("modal-cancel").onclick = () => cleanup(false);
  });
}

async function prompt(message, defaultVal, validator) {
  return new Promise(resolve => {
    const body = `<p>${esc(message)}</p><input id="prompt-input" value="${esc(defaultVal ?? "")}"><div id="prompt-err" class="muted small"></div>`;
    const modal = document.getElementById("modal");
    document.getElementById("modal-title").textContent = "Input";
    document.getElementById("modal-body").innerHTML = body;
    modal.classList.remove("hidden");
    document.getElementById("modal-cancel").style.display = "";
    const input = document.getElementById("prompt-input");
    setTimeout(() => input.focus(), 50);
    const cleanup = (val) => {
      modal.classList.add("hidden");
      document.getElementById("modal-ok").onclick = null;
      document.getElementById("modal-cancel").onclick = null;
      resolve(val);
    };
    document.getElementById("modal-ok").onclick = () => {
      const v = input.value.trim();
      if (validator) {
        const err = validator(v);
        if (err) { document.getElementById("prompt-err").textContent = err; return; }
      }
      cleanup(v);
    };
    document.getElementById("modal-cancel").onclick = () => cleanup(null);
  });
}

async function confirm(message) {
  return infoModal("Confirm", `<p>${esc(message)}</p>`, true);
}

boot().catch(e => {
  toast(`boot failed: ${e.message}`, "error");
  console.error(e);
});
