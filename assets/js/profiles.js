// profiles.js — rail list, wheel editor, inspector, config save.
import { api, el } from "./dom.js";
import { INPUT_SLOTS, INPUT_LABEL } from "./dial.js";

export let config = null;

// Display order: priority desc; "always" (fallback) rules sink to the bottom.
const sorted = () => [...config.rules].sort((a, b) =>
  (a.match?.type === "always") - (b.match?.type === "always")
  || b.priority - a.priority);

const bindingFor = (rule, input) => rule.bindings.find(b => b.input === input);

let currentProfile = -1;
let selectedInput = null;
const slotEls = {};
let onNavigate = () => {};

export function initProfiles(navigate) {
  onNavigate = navigate;
  buildWheel();
  document.getElementById("match-type").onchange = syncMatchProc;
  document.getElementById("use-fg").onclick = useForeground;
  document.getElementById("del-profile").onclick = deleteProfile;
  document.getElementById("add-profile").onclick = addProfile;
  document.getElementById("save-config").onclick = saveConfig;
  document.getElementById("act-type").onchange = () => { applyActionEdit(); syncActValue(); };
  document.getElementById("act-value").oninput = applyActionEdit;
}

export async function loadConfig() {
  const res = await api.get("/api/config");
  if (!res.ok) return;
  config = await res.json();
  document.getElementById("cfg-enabled").checked = config.enabled;
  renderRail();
}

/* ---------- rail ---------- */
export function renderRail() {
  const host = document.getElementById("rail-profiles");
  host.replaceChildren();
  const rules = sorted();
  rules.forEach((rule, i) => {
    const proc = rule.match?.process;
    const b = el("button", { class: "rail-item", type: "button" });
    b.append(
      el("span", { class: "rail-dot" + (rule.match?.type === "always" ? " dim" : "") }),
      el("span", { class: "rail-name" },
        proc && rule.match.type !== "always" ? proc : rule.name));
    const mv = el("span", { class: "rail-mv" });
    for (const [dir, glyph] of [[-1, "▲"], [1, "▼"]]) {
      const btn = el("button", { type: "button" }, glyph);
      const blocked = rule.match?.type === "always"
        || (dir === -1 && (i === 0 || rules[i - 1].match?.type === "always"))
        || (dir === 1 && i === rules.length - 1);
      btn.disabled = blocked;
      btn.onclick = (e) => { e.stopPropagation(); moveRule(rule, dir); };
      mv.append(btn);
    }
    b.append(mv);
    b.onclick = () => onNavigate("profile", i);
    host.append(b);
  });
}

function moveRule(rule, dir) {
  const rules = sorted();
  const i = rules.indexOf(rule);
  const j = i + dir;
  if (j < 0 || j >= rules.length) return;
  const other = rules[j];
  if (other.match?.type === "always") return;
  [rule.priority, other.priority] = [other.priority, rule.priority];
  if (rule.priority === other.priority) rule.priority += -dir;
  renderRail();
  if (currentPageIsProfile()) onNavigate("profile", j);
}
const currentPageIsProfile = () => !document.getElementById("page-profile").hidden;

/* ---------- wheel ---------- */
function buildWheel() {
  const slotsHost = document.getElementById("wheel-slots");
  const rotHost = document.getElementById("wheel-rotate");
  slotsHost.replaceChildren();
  rotHost.replaceChildren();
  for (const [id, label, x, y] of INPUT_SLOTS) {
    const chip = el("button", {
      class: "slot", type: "button", "data-input": id, title: label,
    }, label);
    chip.onclick = () => selectInput(id);
    if (x === null) {
      chip.classList.add("slot-rot");
      rotHost.append(chip);
    } else {
      chip.classList.add(
        id === "knob_click" ? "slot-knob"
        : id.startsWith("button_top") ? "slot-top"
        : "slot-side");
      chip.style.left = x + "%";
      chip.style.top = y + "%";
      slotsHost.append(chip);
    }
    slotEls[id] = chip;
  }
}

function refreshWheelDots() {
  const rule = sorted()[currentProfile];
  for (const [id] of INPUT_SLOTS) {
    const n = slotEls[id];
    if (!n) continue;
    const b = rule && bindingFor(rule, id);
    n.classList.toggle("bound", !!b && b.action?.type !== "none");
  }
}

// Feed hook: flash the slot when its physical input fires.
export function flashSlot(id) {
  const n = slotEls[id];
  if (!n) return;
  n.classList.add("fire");
  setTimeout(() => n.classList.remove("fire"), 220);
}

/* ---------- inspector ---------- */
export function renderProfilePage(idx) {
  const rules = sorted();
  currentProfile = idx;
  const rule = rules[idx];
  if (!rule) return false;
  document.getElementById("prof-name").value = rule.name;
  document.getElementById("match-type").value = rule.match?.type || "always";
  document.getElementById("match-proc").value = rule.match?.process || "";
  syncMatchProc();
  if (!selectedInput || !INPUT_LABEL[selectedInput]) selectedInput = "rotate_cw";
  selectInput(selectedInput);
  refreshWheelDots();
  return true;
}

function syncMatchProc() {
  const t = document.getElementById("match-type").value;
  document.getElementById("match-proc").style.display = t === "always" ? "none" : "";
  document.getElementById("use-fg").style.display = t === "always" ? "none" : "";
}

function renderInspector() {
  const rule = sorted()[currentProfile];
  const editor = document.getElementById("insp-editor");
  const hint = document.getElementById("insp-hint");
  const title = document.getElementById("insp-title");
  if (!rule || !selectedInput) {
    editor.hidden = true;
    hint.hidden = false;
    title.textContent = "Select a dial input";
    return;
  }
  title.textContent = INPUT_LABEL[selectedInput];
  editor.hidden = false;
  hint.hidden = true;
  const act = bindingFor(rule, selectedInput)?.action || { type: "none" };
  document.getElementById("act-type").value = act.type || "none";
  const val = document.getElementById("act-value");
  val.value = act.type === "keys" ? (act.keys || []).join("+")
    : act.type === "command" ? (act.name || "") : "";
  syncActValue();
}

function syncActValue() {
  const t = document.getElementById("act-type").value;
  const val = document.getElementById("act-value");
  val.disabled = t === "none";
  val.placeholder = t === "keys" ? "keys, e.g. ctrl+alt+j"
    : t === "command" ? "command name" : "—";
}

function applyActionEdit() {
  const rule = sorted()[currentProfile];
  if (!rule || !selectedInput) return;
  const type = document.getElementById("act-type").value;
  const raw = document.getElementById("act-value").value;
  let binding = bindingFor(rule, selectedInput);
  if (type === "none" && !binding) return;
  if (!binding) {
    binding = { input: selectedInput, action: { type: "none" } };
    rule.bindings.push(binding);
  }
  if (type === "none") binding.action = { type: "none" };
  else if (type === "keys")
    binding.action = { type: "keys", keys: raw.split("+").map(s => s.trim()).filter(Boolean) };
  else binding.action = { type: "command", name: raw.trim() };
  refreshWheelDots();
}

/* ---------- meta controls ---------- */
async function useForeground() {
  const res = await api.get("/api/foreground");
  const fg = res.ok ? (await res.json()).foreground : null;
  if (fg) document.getElementById("match-proc").value = fg;
}

function deleteProfile() {
  const rule = sorted()[currentProfile];
  if (!rule) return;
  config.rules.splice(config.rules.indexOf(rule), 1);
  renderRail();
  onNavigate("device");
}

function addProfile() {
  config.rules.push({
    name: "new profile", priority: 0,
    match: { type: "foreground", process: "" }, bindings: [],
  });
  renderRail();
  onNavigate("profile", sorted().length - 1);
}

/* ---------- save ---------- */
export async function saveConfig() {
  const status = document.getElementById("save-status");
  status.className = "";
  status.textContent = "saving…";
  config.enabled = document.getElementById("cfg-enabled").checked;
  const rule = sorted()[currentProfile];
  if (currentPageIsProfile() && rule) {
    rule.name = document.getElementById("prof-name").value.trim() || rule.name;
    const mt = document.getElementById("match-type").value;
    rule.match = mt === "always" ? { type: "always" }
      : { type: mt, process: document.getElementById("match-proc").value.trim().toLowerCase() };
  }
  // Priorities follow rail order: topmost wins.
  sorted().forEach((r, i) => { r.priority = sorted().length - i; });
  const res = await api.put("/api/config", config);
  const body = await res.json().catch(() => ({}));
  status.className = body.saved ? "ok" : "err";
  status.textContent = body.saved ? "saved" : (body.error || `HTTP ${res.status}`);
  if (body.saved) { renderRail(); refreshWheelDots(); }
}

/* ---------- import/export ---------- */
export function initConfigPage() {
  document.getElementById("export-config").onclick = () => {
    const blob = new Blob([JSON.stringify(config, null, 2)], { type: "application/json" });
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = "ulanzi-d100h-soft.config.json";
    a.click();
    URL.revokeObjectURL(a.href);
  };
  document.getElementById("import-apply").onclick = async () => {
    const text = document.getElementById("import-text").value;
    const status = document.getElementById("import-status");
    const res = await api.put("/api/config", text);
    const body = await res.json().catch(() => ({}));
    status.className = body.saved ? "ok" : "err";
    status.textContent = body.saved ? "imported" : (body.error || "invalid JSON");
    if (body.saved) {
      config = JSON.parse(text);
      document.getElementById("cfg-enabled").checked = config.enabled;
      renderRail();
      refreshWheelDots();
      document.getElementById("import-text").value = "";
    }
  };
  // Process autocomplete for the match-process input.
  api.get("/api/processes")
    .then(r => r.ok ? r.json() : [])
    .then(names => {
      const list = document.getElementById("proc-list");
      names.filter(n => n.endsWith(".exe")).forEach(n => {
        const o = document.createElement("option");
        o.value = n;
        list.append(o);
      });
    })
    .catch(() => {});
}
