// device.js — Device page: device info card, control row, query buttons.
import { api } from "./dom.js";

export const di = {
  firmware: document.getElementById("di-firmware"),
  serial: document.getElementById("di-serial"),
  address: document.getElementById("di-address"),
  haptics: document.getElementById("di-haptics"),
  sleep: document.getElementById("di-sleep"),
  battery: document.getElementById("di-battery"),
};

export function renderDevice(d) {
  if (!d) return;
  di.firmware.textContent = d.firmware || "—";
  di.serial.textContent = d.serial || "—";
  di.address.textContent = d.address || "—";
  di.haptics.textContent = d.haptics_enabled == null
    ? "—" : (d.haptics_enabled ? "on" : "off");
  di.sleep.textContent = d.sleep_timer_seconds == null
    ? "—" : (d.sleep_timer_seconds + " s");
}

// Replies pushed through the feed keep the card in sync without polling.
export function onReply(entry) {
  if (entry.reply === "firmware_version") di.firmware.textContent = entry.version;
  if (entry.reply === "serial") di.serial.textContent = entry.serial;
  if (entry.reply === "device_address") di.address.textContent = entry.address;
  if (entry.reply === "haptics") di.haptics.textContent = entry.enabled ? "on" : "off";
  if (entry.reply === "sleep_timer") di.sleep.textContent = entry.seconds + " s";
}

export function onBattery(entry) {
  di.battery.textContent = `${entry.percent}%${entry.charging ? " (charging)" : ""}`;
}

export function initDeviceControls(getWs) {
  document.getElementById("reconnect").addEventListener("click", () => {
    const ws = getWs();
    if (ws && ws.readyState === WebSocket.OPEN)
      ws.send(JSON.stringify({ type: "reconnect" }));
    else api.post("/api/reconnect", {});
  });
  document.querySelectorAll("[data-query]").forEach((btn) => {
    btn.addEventListener("click", () => api.wsQuery(getWs(), btn.dataset.query));
  });

  document.getElementById("ctl-apply").onclick = async () => {
    const haptics = document.getElementById("ctl-haptics").checked;
    const sleep = document.getElementById("ctl-sleep").value;
    await api.post("/api/command", { name: "set_haptics", value: haptics });
    if (sleep !== "")
      await api.post("/api/command", { name: "set_sleep_timer", value: Number(sleep) });
  };

  // Reflect device state into the controls when replies arrive.
  const sync = () => {
    if (di.haptics.textContent === "on" || di.haptics.textContent === "off")
      document.getElementById("ctl-haptics").checked = di.haptics.textContent === "on";
    const secs = parseInt(di.sleep.textContent, 10);
    if (!Number.isNaN(secs)) {
      const sel = document.getElementById("ctl-sleep");
      if ([...sel.options].some(o => Number(o.value) === secs)) sel.value = String(secs);
    }
  };
  new MutationObserver(sync).observe(di.haptics, { childList: true });
  new MutationObserver(sync).observe(di.sleep, { childList: true });
}
