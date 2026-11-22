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
    }
  }

  function applySnapshot(data) {
    setStatus(data.status);
    renderDevice(data.device);
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
})();
