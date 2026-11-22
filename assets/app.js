// ulanzi-d100h-soft web UI client.
// Token arrives once as ?token=… in the tray-opened URL; keep it in
// sessionStorage and strip it from the address bar.
(function () {
  const params = new URLSearchParams(location.search);
  let token = params.get("token") || sessionStorage.getItem("ulanzi_token") || "";
  if (params.get("token")) {
    sessionStorage.setItem("ulanzi_token", token);
    params.delete("token");
    const url = location.pathname + (params.size ? "?" + params.toString() : "");
    history.replaceState(null, "", url);
  }

  const statusEl = document.getElementById("status");
  const feedEl = document.getElementById("feed");
  const di = {
    firmware: document.getElementById("di-firmware"),
    serial: document.getElementById("di-serial"),
    address: document.getElementById("di-address"),
    haptics: document.getElementById("di-haptics"),
    sleep: document.getElementById("di-sleep"),
    battery: document.getElementById("di-battery"),
  };
  const MAX_ROWS = 200;

  function setStatus(status) {
    statusEl.textContent = status;
    statusEl.dataset.status = status;
  }

  function renderDevice(d) {
    if (!d) return;
    di.firmware.textContent = d.firmware || "—";
    di.serial.textContent = d.serial || "—";
    di.address.textContent = d.address || "—";
    di.haptics.textContent = d.haptics_enabled === null || d.haptics_enabled === undefined
      ? "—" : (d.haptics_enabled ? "on" : "off");
    di.sleep.textContent = d.sleep_timer_seconds == null
      ? "—" : (d.sleep_timer_seconds + " s");
  }

  function describe(body) {
    switch (body.subtype) {
      case "event": {
        const e = body;
        if (e.event === "button") return `${e.button} ${e.pressed ? "down" : "up"}`;
        if (e.event === "knob_click") return `knob ${e.pressed ? "down" : "up"}`;
        if (e.event === "rotation")
          return `rotate ${e.direction}${e.knob_pressed ? " (held)" : ""}`;
        return JSON.stringify(e);
      }
      case "reply": {
        const r = body;
        if (r.reply === "firmware_version") return `firmware ${r.version}`;
        if (r.reply === "device_address") return `address ${r.address}`;
        if (r.reply === "serial") return `serial ${r.serial}`;
        if (r.reply === "haptics") return `haptics ${r.enabled ? "on" : "off"}${r.write_response ? " (write ack)" : ""}`;
        if (r.reply === "sleep_timer") return `sleep timer ${r.seconds} s${r.write_response ? " (write ack)" : ""}`;
        if (r.reply === "heartbeat") return "heartbeat";
        return JSON.stringify(r);
      }
      case "battery": {
        const b = body;
        return `battery ${b.percent}%${b.charging ? " (charging)" : ""}${b.charge_full ? " (full)" : ""}`;
      }
      case "unknown":
        return `unknown ${body.hex}`;
      case "status":
        return `status ${body.status}${body.detail ? " — " + body.detail : ""}`;
      default:
        return JSON.stringify(body);
    }
  }

  function appendEntry(entry) {
    // Body fields are flattened into entry by serde (no entry.body wrapper).
    const li = document.createElement("li");
    const time = document.createElement("time");
    time.textContent = new Date(entry.at).toLocaleTimeString();
    const span = document.createElement("span");
    span.textContent = describe(entry);
    span.className = "feed-" + entry.subtype;
    li.append(time, span);
    feedEl.prepend(li);
    while (feedEl.children.length > MAX_ROWS) feedEl.lastElementChild.remove();

    // Keep the device card in sync with replies/battery without extra calls.
    if (entry.subtype === "reply") {
      if (entry.reply === "firmware_version") di.firmware.textContent = entry.version;
      if (entry.reply === "serial") di.serial.textContent = entry.serial;
      if (entry.reply === "device_address") di.address.textContent = entry.address;
      if (entry.reply === "haptics") di.haptics.textContent = entry.enabled ? "on" : "off";
      if (entry.reply === "sleep_timer") di.sleep.textContent = entry.seconds + " s";
    } else if (entry.subtype === "battery") {
      di.battery.textContent = `${entry.percent}%${entry.charging ? " (charging)" : ""}`;
    } else if (entry.subtype === "status") {
      setStatus(entry.status);
      const m = /rule "(.+)" active/.exec(entry.detail || "");
      if (m) document.getElementById("active-rule").textContent = "rule: " + m[1];
      const f = /foreground: (.+)/.exec(entry.detail || "");
      if (f) document.getElementById("fg-app").textContent = f[1];
    }
  }

  function applySnapshot(data) {
    setStatus(data.status);
    renderDevice(data.device);
    document.getElementById("active-rule").textContent =
      "rule: " + (data.active_rule || "—");
    feedEl.replaceChildren();
    for (const entry of data.feed) appendEntry(entry);
  }

  let ws;
  let backoff = 500;
  function connect() {
    ws = new WebSocket(`ws://${location.host}/ws?token=${encodeURIComponent(token)}`);
    ws.onopen = () => { backoff = 500; };
    ws.onmessage = (m) => {
      let frame;
      try { frame = JSON.parse(m.data); } catch { return; }
      if (frame.type === "snapshot") applySnapshot(frame.data);
      else if (frame.type === "entry") appendEntry(frame.data);
      else if (frame.type === "status") setStatus(frame.data);
    };
    ws.onclose = () => {
      setStatus("disconnected");
      setTimeout(connect, backoff);
      backoff = Math.min(backoff * 2, 10000);
    };
  }

  document.getElementById("reconnect").addEventListener("click", () => {
    if (ws && ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify({ type: "reconnect" }));
    else fetch(`/api/reconnect?token=${encodeURIComponent(token)}`, { method: "POST" });
  });
  document.querySelectorAll("[data-query]").forEach((btn) => {
    btn.addEventListener("click", () => {
      const name = btn.dataset.query;
      if (ws && ws.readyState === WebSocket.OPEN)
        ws.send(JSON.stringify({ type: "query", name }));
      else
        fetch(`/api/query?token=${encodeURIComponent(token)}&name=${name}`);
    });
  });

  if (!token) setStatus("no token — open from the tray icon");
  connect();

/* ---- App rules editor ---- */

const INPUT_IDS = [
  ["button_top_left", "Button top-left"],
  ["button_top_middle", "Button top-middle"],
  ["button_top_right", "Button top-right"],
  ["button_upper_left", "Button upper-left"],
  ["button_upper_right", "Button upper-right"],
  ["button_lower_left", "Button lower-left"],
  ["button_lower_right", "Button lower-right"],
  ["knob_click", "Knob click"],
  ["rotate_cw", "Rotate clockwise"],
  ["rotate_ccw", "Rotate counter-clockwise"],
  ["rotate_held_cw", "Rotate held clockwise"],
  ["rotate_held_ccw", "Rotate held counter-clockwise"],
];
const MATCH_TYPES = [
  ["foreground", "foreground app"],
  ["running", "process running"],
  ["not_running", "process NOT running"],
  ["always", "always (default)"],
];

let config = null;

function el(tag, attrs = {}, ...children) {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k === "class") node.className = v;
    else if (k === "type") node.type = v;
    else if (k === "checked") node.checked = v;
    else node.setAttribute(k, v);
  }
  node.append(...children);
  return node;
}

function renderRules() {
  const host = document.getElementById("rules");
  host.replaceChildren();
  const rules = [...config.rules].sort((a, b) => b.priority - a.priority);
  for (const rule of rules) {
    const idx = config.rules.indexOf(rule);
    const card = el("div", { class: "rule" });

    const name = el("input", { type: "text", value: rule.name });
    const prio = el("input", { type: "number", value: rule.priority, step: "1" });
    const matchSel = el("select");
    for (const [v, label] of MATCH_TYPES) {
      matchSel.append(el("option", { value: v }, label));
    }
    matchSel.value = rule.match.type;
    const procInput = el("input", {
      type: "text", value: rule.match.process || "", placeholder: "process.exe",
      list: "proc-list",
      style: rule.match.type === "always" ? "display:none" : "",
    });
    matchSel.onchange = () => {
      procInput.style.display = matchSel.value === "always" ? "none" : "";
    };
    const useFg = el("button", { type: "button", title: "use current foreground app" }, "use fg");
    useFg.onclick = async () => {
      const fg = await currentForeground();
      if (fg) {
        matchSel.value = "foreground";
        procInput.style.display = "";
        procInput.value = fg;
      }
    };
    const del = el("button", { class: "del", type: "button" }, "delete");
    del.onclick = () => { config.rules.splice(idx, 1); renderRules(); };
    card.append(el("div", { class: "rule-head" },
      name, el("span", { class: "hint" }, "prio"), prio, matchSel, procInput, useFg, del));

    const bindings = el("div", { class: "bindings" });
    const usedInputs = new Set(rule.bindings.map(b => b.input));
    for (const binding of rule.bindings) {
      bindings.append(bindingRow(rule, binding));
    }
    const addSel = el("select");
    addSel.append(el("option", { value: "" }, "+ bind input…"));
    for (const [id, label] of INPUT_IDS) {
      if (!usedInputs.has(id)) addSel.append(el("option", { value: id }, label));
    }
    addSel.onchange = () => {
      if (!addSel.value) return;
      rule.bindings.push({ input: addSel.value, action: { type: "none" } });
      renderRules();
    };
    bindings.append(addSel);
    card.append(bindings);
    host.append(card);

    rule._ui = { name, prio, matchSel, procInput };
  }
}

function bindingRow(rule, binding) {
  const row = el("div", { class: "binding-row" });
  const sel = el("select");
  for (const [id, label] of INPUT_IDS) {
    sel.append(el("option", { value: id }, label));
  }
  sel.value = binding.input;
  sel.onchange = () => { binding.input = sel.value; };

  const actSel = el("select");
  for (const [v, label] of [["none", "none"], ["keys", "key chord"], ["command", "command"]]) {
    actSel.append(el("option", { value: v }, label));
  }
  actSel.value = binding.action.type;

  const valueInput = el("input", { type: "text", placeholder: "keys, e.g. ctrl+alt+j" });
  const syncValue = () => {
    if (actSel.value === "keys") {
      valueInput.value = (binding.action.keys || []).join("+");
      valueInput.style.display = "";
    } else if (actSel.value === "command") {
      valueInput.value = binding.action.name || "";
      valueInput.style.display = "";
    } else {
      valueInput.style.display = "none";
    }
  };
  syncValue();
  actSel.onchange = () => {
    binding.action = { type: actSel.value };
    if (actSel.value === "keys") binding.action.keys = [];
    if (actSel.value === "command") binding.action.name = "";
    syncValue();
  };
  valueInput.oninput = () => {
    if (actSel.value === "keys") {
      binding.action.keys = valueInput.value.split("+").map(s => s.trim()).filter(Boolean);
    } else if (actSel.value === "command") {
      binding.action.name = valueInput.value;
    }
  };

  const del = el("button", { class: "del", type: "button" }, "×");
  del.onclick = () => {
    rule.bindings.splice(rule.bindings.indexOf(binding), 1);
    renderRules();
  };
  row.append(sel, actSel, valueInput, del);
  return row;
}

async function saveConfig() {
  const status = document.getElementById("save-status");
  status.className = "";
  status.textContent = "saving…";
  config.enabled = document.getElementById("cfg-enabled").checked;
  for (const rule of config.rules) {
    if (!rule._ui) continue;
    rule.name = rule._ui.name.value;
    rule.priority = parseInt(rule._ui.prio.value, 10) || 0;
    const type = rule._ui.matchSel.value;
    rule.match = type === "always"
      ? { type }
      : { type, process: rule._ui.procInput.value.trim().toLowerCase() };
    delete rule._ui;
  }
  const res = await fetch(`/api/config?token=${encodeURIComponent(token)}`, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(config),
  });
  const body = await res.json().catch(() => ({}));
  if (body.saved) {
    status.className = "ok";
    status.textContent = "saved";
    renderRules();
  } else {
    status.className = "err";
    status.textContent = body.error || `HTTP ${res.status}`;
  }
}

async function initRules() {
  const res = await fetch(`/api/config?token=${encodeURIComponent(token)}`);
  if (!res.ok) return;
  config = await res.json();
  document.getElementById("cfg-enabled").checked = config.enabled;
  document.getElementById("add-rule").onclick = () => {
    config.rules.push({ name: "new rule", priority: 0, match: { type: "always" }, bindings: [] });
    renderRules();
  };
  document.getElementById("save-config").onclick = saveConfig;
  renderRules();
}
initRules();

/* ---- Advanced controls ---- */

// Feed pause/clear.
let feedPaused = false;
document.getElementById("feed-pause").onclick = (e) => {
  feedPaused = !feedPaused;
  e.target.textContent = feedPaused ? "resume" : "pause";
};
document.getElementById("feed-clear").onclick = () => feedEl.replaceChildren();

// Wrap appendEntry: pause drops live entries (they're in the feed buffer
// server-side anyway; the next snapshot resyncs).
const origAppend = appendEntry;
appendEntry = (entry) => { if (!feedPaused) origAppend(entry); };

// Device controls: send writes through /api/command.
document.getElementById("ctl-apply").onclick = async () => {
  const haptics = document.getElementById("ctl-haptics").checked;
  const sleep = document.getElementById("ctl-sleep").value;
  await fetch(`/api/command?token=${encodeURIComponent(token)}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ name: "set_haptics", value: haptics }),
  });
  if (sleep !== "") {
    await fetch(`/api/command?token=${encodeURIComponent(token)}`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name: "set_sleep_timer", value: Number(sleep) }),
    });
  }
};
// Reflect device state into the controls when replies arrive.
const syncControls = () => {
  if (di.haptics.textContent === "on" || di.haptics.textContent === "off")
    document.getElementById("ctl-haptics").checked = di.haptics.textContent === "on";
  const secs = parseInt(di.sleep.textContent, 10);
  if (!Number.isNaN(secs)) {
    const sel = document.getElementById("ctl-sleep");
    if ([...sel.options].some(o => Number(o.value) === secs)) sel.value = String(secs);
  }
};
new MutationObserver(syncControls).observe(di.haptics, { childList: true });
new MutationObserver(syncControls).observe(di.sleep, { childList: true });

// Process autocomplete for match process inputs.
fetch(`/api/processes?token=${encodeURIComponent(token)}`)
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

// "use foreground" button per rule — filled in renderRules below.
async function currentForeground() {
  const res = await fetch(`/api/foreground?token=${encodeURIComponent(token)}`);
  return res.ok ? (await res.json()).foreground : null;
}

// Export: download config.json.
document.getElementById("export-config").onclick = () => {
  const blob = new Blob([JSON.stringify(config, null, 2)], { type: "application/json" });
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = "ulanzi-d100h-soft.config.json";
  a.click();
  URL.revokeObjectURL(a.href);
};

// Import: paste JSON, reuse the PUT path for validation.
document.getElementById("import-config").onclick = () => {
  const box = document.getElementById("import-box");
  box.hidden = !box.hidden;
};
document.getElementById("import-apply").onclick = async () => {
  const text = document.getElementById("import-text").value;
  const status = document.getElementById("save-status");
  const res = await fetch(`/api/config?token=${encodeURIComponent(token)}`, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: text,
  });
  const body = await res.json().catch(() => ({}));
  if (body.saved) {
    status.className = "ok";
    status.textContent = "imported";
    config = JSON.parse(text);
    document.getElementById("cfg-enabled").checked = config.enabled;
    renderRules();
    document.getElementById("import-box").hidden = true;
  } else {
    status.className = "err";
    status.textContent = body.error || "invalid JSON";
  }
};
})();
