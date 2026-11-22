// dial.js — dial SVG decoration (ticks, grip) and input<->slot model,
// shared by the device-page dial and the profile wheel editor.

const SVG_NS = "http://www.w3.org/2000/svg";

export function drawTicks(el, count = 60, r1 = 84, r2 = 88, majorEvery = 5) {
  if (!el) return;
  for (let i = 0; i < count; i++) {
    const a = (i / count) * Math.PI * 2;
    const major = i % majorEvery === 0;
    const l = document.createElementNS(SVG_NS, "line");
    const s = major ? r1 - 4 : r1;
    l.setAttribute("x1", 100 + s * Math.cos(a));
    l.setAttribute("y1", 100 + s * Math.sin(a));
    l.setAttribute("x2", 100 + r2 * Math.cos(a));
    l.setAttribute("y2", 100 + r2 * Math.sin(a));
    l.setAttribute("stroke", major ? "#4b5a70" : "#2c3746");
    l.setAttribute("stroke-width", major ? "1.6" : "1");
    el.append(l);
  }
}

export function drawGrip(el, count = 24) {
  if (!el) return;
  for (let i = 0; i < count; i++) {
    const a = (i / count) * Math.PI * 2;
    const l = document.createElementNS(SVG_NS, "line");
    l.setAttribute("x1", 100 + 30 * Math.cos(a));
    l.setAttribute("y1", 100 + 30 * Math.sin(a));
    l.setAttribute("x2", 100 + 34 * Math.cos(a));
    l.setAttribute("y2", 100 + 34 * Math.sin(a));
    l.setAttribute("stroke", "#35415a");
    l.setAttribute("stroke-width", "2");
    el.append(l);
  }
}

// Spin marker inside an SVG (element id "dial-marker" with a shape at top).
export function makeSpinner(markerEl) {
  let angle = 0;
  return (direction) => {
    angle += direction === "Clockwise" ? 12 : -12;
    if (markerEl) markerEl.setAttribute("transform", `rotate(${angle} 100 100)`);
  };
}

// Dial inputs: [id, label, x%, y% within the wheel area].
// Physical pad layout: three buttons above the dial, two pairs flanking
// it on the left/right, knob in the dial center.
export const INPUT_SLOTS = [
  ["button_top_left", "top-L", 38, 10],
  ["button_top_middle", "top", 50, 7],
  ["button_top_right", "top-R", 62, 10],
  ["button_upper_left", "up-L", 15, 38],
  ["button_lower_left", "low-L", 15, 60],
  ["button_upper_right", "up-R", 85, 38],
  ["button_lower_right", "low-R", 85, 60],
  ["knob_click", "knob", 50, 49],
  ["rotate_cw", "rotate ↻", null],
  ["rotate_ccw", "rotate ↺", null],
  ["rotate_held_cw", "held ↻", null],
  ["rotate_held_ccw", "held ↺", null],
];

export const INPUT_LABEL = Object.fromEntries(INPUT_SLOTS.map(([id, l]) => [id, l]));
export function eventInputId(e) {
  if (e.event === "rotation")
    return e.knob_pressed
      ? (e.direction === "Clockwise" ? "rotate_held_cw" : "rotate_held_ccw")
      : (e.direction === "Clockwise" ? "rotate_cw" : "rotate_ccw");
  if (e.event === "knob_click") return "knob_click";
  if (e.event === "button") return "button_" + e.button;
  return null;
}
