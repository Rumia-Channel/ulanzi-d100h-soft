// main.js — navigation, WebSocket, wiring the modules together.
import { token, setBadge } from "./dom.js";
import { drawTicks, drawGrip, makeSpinner, eventInputId } from "./dial.js";
import { createFeed } from "./feed.js";
import { renderDevice, onReply, onBattery, initDeviceControls } from "./device.js";
import {
  loadConfig, initProfiles, initConfigPage, renderProfilePage, flashSlot,
} from "./profiles.js";

/* ---- navigation ---- */
const pages = {
  device: document.getElementById("page-device"),
  profile: document.getElementById("page-profile"),
  config: document.getElementById("page-config"),
};
let profileIdx = -1;

function navTo(page, idx = -1) {
  profileIdx = idx;
  for (const [k, n] of Object.entries(pages)) n.hidden = k !== page;
  if (page === "profile" && !renderProfilePage(idx)) {
    page = "device";
    pages.profile.hidden = true;
    pages.device.hidden = false;
  }
  document.getElementById("nav-device").classList.toggle("sel", page === "device");
  document.getElementById("nav-config").classList.toggle("sel", page === "config");
  [...document.querySelectorAll("#rail-profiles .rail-item")].forEach((b, i) =>
    b.classList.toggle("sel", page === "profile" && i === idx));
}
document.getElementById("nav-device").onclick = () => navTo("device");
document.getElementById("nav-config").onclick = () => navTo("config");

/* ---- live visuals shared by pages ---- */
const bumpDial = makeSpinner(document.getElementById("dial-marker"));

/* ---- feed ---- */
const feed = createFeed(document.getElementById("feed"), {
  onEvent: (e) => {
    if (e.event === "rotation") bumpDial(e.direction);
    flashSlot(eventInputId(e));
  },
  onReply,
  onBattery,
  onStatus: (status, detail) => {
    setBadge("status", status, status);
    const m = /rule "(.+)" active/.exec(detail);
    if (m) setBadge("active-rule", "rule: " + m[1]);
    const f = /foreground: (.+)/.exec(detail);
    if (f) setBadge("fg-app", f[1]);
  },
});
document.getElementById("feed-pause").onclick = (e) => feed.togglePause(e.target);
document.getElementById("feed-clear").onclick = () => feed.clear();

/* ---- websocket ---- */
let ws;
let backoff = 500;
function connect() {
  ws = new WebSocket(`ws://${location.host}/ws?token=${encodeURIComponent(token)}`);
  ws.onopen = () => { backoff = 500; };
  ws.onmessage = (m) => {
    let frame;
    try { frame = JSON.parse(m.data); } catch { return; }
    if (frame.type === "snapshot") {
      setBadge("status", frame.data.status, frame.data.status);
      renderDevice(frame.data.device);
      setBadge("active-rule", "rule: " + (frame.data.active_rule || "—"));
      feed.replay(frame.data.feed);
    } else if (frame.type === "entry") feed.push(frame.data);
    else if (frame.type === "status") setBadge("status", frame.data, frame.data);
  };
  ws.onclose = () => {
    setBadge("status", "disconnected", "disconnected");
    setTimeout(connect, backoff);
    backoff = Math.min(backoff * 2, 10000);
  };
}

/* ---- boot ---- */
drawTicks(document.getElementById("dial-ticks"));
drawTicks(document.getElementById("wheel-ticks"));
drawGrip(document.getElementById("dial-grip"));

initProfiles(navTo);
initConfigPage();
initDeviceControls(() => ws);
loadConfig();

if (!token) setBadge("status", "no token — open from the tray icon");
connect();
