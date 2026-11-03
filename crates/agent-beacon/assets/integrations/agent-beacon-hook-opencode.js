// @agent-beacon-integration: opencode
// @agent-beacon-version: __VERSION__
// Managed by agent-beacon. Run `agent-beacon integrate upgrade` to update.

const URL = process.env.AGENT_BEACON_URL || "http://127.0.0.1:6980";
const AGENT = "opencode";

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

export const AgentBeaconPlugin = async () => {
  return {
    event: async ({ event }) => {
      const session =
        (event && event.properties && event.properties.sessionID) ||
        (event && event.sessionID);
      switch (event && event.type) {
        case "session.created":
          report("idle", undefined, undefined, session);
          break;
        case "permission.asked":
          report("waiting", "等待权限确认", undefined, session);
          break;
        case "session.error":
          report("error", "会话出错", undefined, session);
          break;
        case "session.idle":
          report("idle", undefined, undefined, session);
          break;
        case "session.deleted":
          report("idle", undefined, 30, session);
          break;
        default:
          break;
      }
    },
    "tool.execute.before": async (input) => {
      report("working", input && input.tool);
    },
  };
};
