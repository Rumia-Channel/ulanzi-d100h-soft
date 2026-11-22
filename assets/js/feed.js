// feed.js — event feed rendering + per-entry side effects hook.
// onEvent(entry) is invoked for live visuals (dial spin, slot flash);
// onStatus(status, detail) for header badges.

const MAX_ROWS = 300;

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

export function createFeed(feedEl, { onEvent, onReply, onBattery, onStatus } = {}) {
  let paused = false;
  const push = (entry) => {
    if (paused) return;
    const li = document.createElement("li");
    const time = document.createElement("time");
    time.textContent = new Date(entry.at).toLocaleTimeString();
    const span = document.createElement("span");
    span.textContent = describe(entry);
    span.className = "feed-" + entry.subtype;
    li.append(time, span);
    feedEl.prepend(li);
    while (feedEl.children.length > MAX_ROWS) feedEl.lastElementChild.remove();

    if (entry.subtype === "event") onEvent?.(entry);
    else if (entry.subtype === "reply") onReply?.(entry);
    else if (entry.subtype === "battery") onBattery?.(entry);
    else if (entry.subtype === "status") onStatus?.(entry.status, entry.detail || "");
  };
  return {
    push,
    clear: () => feedEl.replaceChildren(),
    replay: (entries) => { feedEl.replaceChildren(); entries.forEach(push); },
    togglePause: (btn) => {
      paused = !paused;
      if (btn) btn.textContent = paused ? "resume" : "pause";
      return paused;
    },
  };
}
