// dom.js — shared DOM helper, token handling, API client.

export const token = (() => {
  let t = new URLSearchParams(location.search).get("token");
  if (t) {
    sessionStorage.setItem("dial-token", t);
    history.replaceState(null, "", location.pathname);
  } else {
    t = sessionStorage.getItem("dial-token") || "";
  }
  return t;
})();

// el("div", {class, ...attrs}, ...children)
export function el(tag, attrs = {}, ...children) {
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

const q = (path, init) =>
  fetch(`${path}${path.includes("?") ? "&" : "?"}token=${encodeURIComponent(token)}`, init);

export const api = {
  get: (path) => q(path),
  post: (path, body) => q(path, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  }),
  put: (path, body) => q(path, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: typeof body === "string" ? body : JSON.stringify(body),
  }),
  // Over WS when open (lower latency), else HTTP.
  wsQuery(ws, name) {
    if (ws && ws.readyState === WebSocket.OPEN)
      ws.send(JSON.stringify({ type: "query", name }));
    else api.get(`/api/query?name=${name}`);
  },
};

export function setBadge(id, text, status) {
  const n = document.getElementById(id);
  n.textContent = text;
  if (status !== undefined) n.dataset.status = status;
}
