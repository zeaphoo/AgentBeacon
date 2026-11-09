// @agent-beacon-integration: pi
// @agent-beacon-version: __VERSION__
// Managed by agent-beacon. Run `agent-beacon integrate upgrade` to update.

const URL = process.env.AGENT_BEACON_URL || "http://127.0.0.1:6980";
const AGENT = "pi";

// Hook 事件可能连续触发（agent_start → tool_call → agent_settled），而 fetch
// 是异步的。这里用 latest-wins 队列串行发送，既保证服务端看到的顺序与 Pi
// 事件顺序一致，也避免服务不可达时堆积一长串过期状态。
let pending = null;
let sending = false;

function report(state, message, ttl, session) {
  if (typeof fetch !== "function") return;
  pending = { state, message, ttl, session };
  if (!sending) void drain();
}

async function drain() {
  if (sending) return;
  sending = true;
  try {
    while (pending) {
      const job = pending;
      pending = null;
      await send(job);
    }
  } finally {
    sending = false;
  }
}

async function send(job) {
  const body = { agent: AGENT, state: job.state };
  if (job.message) body.message = String(job.message).slice(0, 1024);
  if (job.ttl) body.ttl_secs = Number(job.ttl);
  if (typeof job.session === "string" && job.session) body.session = job.session;

  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 900);
  try {
    await fetch(`${URL}/api/v1/events`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
      signal: controller.signal,
    });
  } catch {
    // 状态灯是尽力而为的旁路信号，服务不可达时不能影响 Pi。
  } finally {
    clearTimeout(timer);
  }
}

function sessionId(ctx) {
  try {
    const id = ctx && ctx.sessionManager && ctx.sessionManager.getSessionId
      ? ctx.sessionManager.getSessionId()
      : undefined;
    return typeof id === "string" && id.length > 0 ? id : undefined;
  } catch {
    return undefined;
  }
}

function reportIdle(ctx, ttl) {
  // session_start 也会在扩展热重载时触发。此时 Pi 可能仍在运行，
  // 如果无条件上报 idle，灯会先变绿，等下一次 tool_call 才恢复黄色。
  const idle = typeof ctx?.isIdle === "function" ? ctx.isIdle() : true;
  report(idle ? "idle" : "working", undefined, ttl, sessionId(ctx));
}

export default function (pi) {
  pi.on("session_start", (_event, ctx) => reportIdle(ctx));
  // before_agent_start 比 agent_start 更早，能覆盖压缩检查/准备阶段的空档。
  pi.on("before_agent_start", (_event, ctx) =>
    report("working", undefined, undefined, sessionId(ctx)),
  );
  pi.on("agent_start", (_event, ctx) =>
    report("working", undefined, undefined, sessionId(ctx)),
  );
  pi.on("tool_call", (event, ctx) =>
    report("working", event && event.toolName, undefined, sessionId(ctx)),
  );
  pi.on("agent_settled", (_event, ctx) => report("idle", undefined, undefined, sessionId(ctx)));
  // 会话关闭后让状态在 30 秒内自然过期，避免灯长期停留。
  // reload 只是替换扩展，不是会话结束；session_start 会立即重新上报真实状态。
  pi.on("session_shutdown", (event, ctx) => {
    if (event && event.reason === "reload") return;
    report("idle", undefined, 30, sessionId(ctx));
  });
}
