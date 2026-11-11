// @agent-beacon-integration: dsh
// @agent-beacon-version: __VERSION__
// Managed by agent-beacon. Run `agent-beacon integrate upgrade` to update.

const URL = process.env.AGENT_BEACON_URL || "http://127.0.0.1:6980";
const AGENT = "dsh";

function report(state, message, ttl, session, event) {
  if (typeof fetch !== "function") return;
  const body = { agent: AGENT, state };
  if (message) body.message = String(message).slice(0, 1024);
  if (ttl) body.ttl_secs = Number(ttl);
  if (typeof session === "string" && session) body.session = session;
  if (typeof event === "string" && event) body.event = event;

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

function sessionId(agent) {
  try {
    const id = agent && agent.session && agent.session.id;
    return typeof id === "string" ? id : undefined;
  } catch {
    return undefined;
  }
}

function errorText(error) {
  if (!error) return "未知错误";
  if (typeof error === "string") return error;
  if (error.message) return String(error.message);
  return String(error);
}

export const name = "agent-beacon-hook-dsh";

export function apply(ctx) {
  ctx.on("agent/created", ({ agent }) => {
    report("idle", undefined, undefined, sessionId(agent), "agent/created");
  });

  ctx.on("agent/status", ({ agent, status }) => {
    report(
      status === "running" ? "working" : "idle",
      undefined,
      undefined,
      sessionId(agent),
      "agent/status",
    );
  });

  ctx.on("agent/error", ({ agent, error }) => {
    report("error", errorText(error), undefined, sessionId(agent), "agent/error");
  });

  ctx.on("agent/disposed", ({ agent }) => {
    report("idle", undefined, 30, sessionId(agent), "agent/disposed");
  });

  // approval/asked 与 approval/decided 是 session 日志事件。
  ctx.on("session/event", (session, event) => {
    if (!session || !event) return;
    if (event.type === "approval/asked") {
      report("waiting", "等待审批", undefined, session.id, event.type);
    } else if (event.type === "approval/decided") {
      report("working", undefined, undefined, session.id, event.type);
    }
  });
}
