// @agent-beacon-integration: pi
// @agent-beacon-version: __VERSION__
// Managed by agent-beacon. Run `agent-beacon integrate upgrade` to update.

const URL = process.env.AGENT_BEACON_URL || "http://127.0.0.1:6980";
const AGENT = "pi";

function report(state, message, ttl, session) {
  if (typeof fetch !== "function") return;
  const body = { agent: AGENT, state };
  if (message) body.message = String(message).slice(0, 1024);
  if (ttl) body.ttl_secs = Number(ttl);
  if (typeof session === "string" && session) body.session = session;

  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 900);
  fetch(`${URL}/api/v1/events`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
    signal: controller.signal,
  })
    .catch(() => {})
    .finally(() => clearTimeout(timer));
}

export default function (pi) {
  pi.on("session_start", () => report("idle"));
  pi.on("agent_start", () => report("working"));
  pi.on("tool_call", (event) => report("working", event && event.toolName));
  pi.on("agent_settled", () => report("idle"));
  // 会话关闭后让状态在 30 秒内自然过期，避免灯长期停留。
  pi.on("session_shutdown", () => report("idle", undefined, 30));
}
