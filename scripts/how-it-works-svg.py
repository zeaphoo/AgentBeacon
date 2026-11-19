#!/usr/bin/env python3
"""生成 AgentBeacon 工作原理动画（矢量 SVG，SMIL 动画）。

相比 GIF：矢量文字随便放大都清晰，呼吸/闪烁是真正连续的动画，体积约 150 KB。
画面与产品一致：左侧多个 Agent 会话、中间聚合状态与「多灯控制策略」、右侧每台灯分到
的 Agent / 会话与灯效。

    python3 scripts/how-it-works-svg.py --lang zh
"""

from __future__ import annotations

import argparse
import base64
import io
from pathlib import Path
from xml.sax.saxutils import escape

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
ASSETS = ROOT / "web" / "src" / "assets" / "devices"
DOCS_ASSETS = ROOT / "docs" / "assets"

W, H = 960, 560
DURATION = 12.0

BG = "#F7F9FB"
CARD = "#FFFFFF"
BORDER = "#E2E7EC"
TEXT = "#111827"
MUTED = "#6B7280"
FAINT = "#94A3B8"
ORANGE = "#F97316"
GREEN = "#16A34A"
AMBER = "#F59E0B"
RED = "#DC2626"

SOFT = {"green": "#DCFCE7", "orange": "#FFEDD5", "amber": "#FEF3C7", "red": "#FEE2E2"}
STATE_COLOR = {"idle": "green", "working": "orange", "waiting": "amber", "error": "red"}
STATE_RGB = {"green": GREEN, "orange": ORANGE, "amber": AMBER, "red": RED}
EFFECT_OF_STATE = {"idle": "calm", "working": "busy", "waiting": "attention", "error": "alert"}
EFFECT_COLOR = {"calm": GREEN, "busy": ORANGE, "attention": ORANGE, "alert": RED}

FONT_FAMILY = (
    "'Noto Sans CJK SC','Noto Sans SC','Source Han Sans SC','PingFang SC',"
    "'Microsoft YaHei','Helvetica Neue',Arial,sans-serif"
)

LABELS = {
    "zh": {
        "subtitle": "多个 Agent 会话  →  聚合状态  →  多台状态灯",
        "agents": "Agent 会话",
        "hub_sub": "聚合 + 多灯分配",
        "strategies": "多灯控制策略",
        "strategy": {
            "unified": ("统一显示", "所有灯显示同一个全局状态"),
            "auto_agent": ("按 Agent 自动分配", "每个 Agent 自动分到一台灯"),
            "manual_agent": ("按 Agent 手动分配", "手动指定每台灯显示哪些 Agent"),
            "auto_session": ("按 Session 自动分配", "同一个 Agent 的多个会话分散到不同灯"),
        },
        "aggregate": "聚合状态",
        "devices": "状态灯",
        "state": {"idle": "空闲", "working": "运行中", "waiting": "等待确认", "error": "错误"},
        "effect": {
            "calm": "绿色常亮",
            "busy": "橙色呼吸",
            "attention": "橙色常亮",
            "alert": "红色闪烁",
        },
        "global": "全局状态",
        "sessions": "{n} 个会话",
        "manual_tag": "手动",
        "agents_list": ["Claude Code", "Codex CLI", "Pi"],
        "session_labels": ["Claude · 会话 1", "Claude · 会话 2", "Codex · 会话 1", "Pi · 会话 1"],
        "devices_list": [
            ("mlight.png", "MLight 三灯珠", 100),
            ("agentcore-halo.png", "AgentCore-Halo", 100),
            ("agentcore-light.png", "AgentCore-Light", 122),
        ],
        "footnote": "策略、分配结果与灯效映射都能在控制台里按设备调整",
    },
    "en": {
        "subtitle": "Many agent sessions  →  one aggregate state  →  many status lights",
        "agents": "Agent sessions",
        "hub_sub": "aggregate + fan-out",
        "strategies": "Multi-light strategy",
        "strategy": {
            "unified": ("Unified", "All lights show the same global state"),
            "auto_agent": ("Auto by Agent", "Each Agent gets one light automatically"),
            "manual_agent": ("Manual by Agent", "Choose which Agents each light shows"),
            "auto_session": ("Auto by Session", "Sessions of one Agent spread across lights"),
        },
        "aggregate": "Aggregate",
        "devices": "Status lights",
        "state": {"idle": "Idle", "working": "Working", "waiting": "Waiting", "error": "Error"},
        "effect": {
            "calm": "Solid green",
            "busy": "Orange breathing",
            "attention": "Solid orange",
            "alert": "Red blinking",
        },
        "global": "Global state",
        "sessions": "{n} sessions",
        "manual_tag": "manual",
        "agents_list": ["Claude Code", "Codex CLI", "Pi"],
        "session_labels": [
            "Claude · session 1",
            "Claude · session 2",
            "Codex · session 1",
            "Pi · session 1",
        ],
        "devices_list": [
            ("mlight.png", "MLight 3 LEDs", 100),
            ("agentcore-halo.png", "AgentCore-Halo", 100),
            ("agentcore-light.png", "AgentCore-Light", 122),
        ],
        "footnote": "Strategy, assignments and effect mapping are all editable per device",
    },
}

SESSIONS = [(0, 0), (0, 1), (1, 2), (2, 3)]

# 每个会话自身的状态（秒区间）
SESSION_STATES = [
    [(0, 1.0, "idle"), (1.0, 10.4, "working"), (10.4, DURATION, "idle")],
    [(0, 5.4, "idle"), (5.4, 10.4, "working"), (10.4, DURATION, "idle")],
    [(0, 4.0, "idle"), (4.0, 10.4, "error"), (10.4, DURATION, "idle")],
    [(0, 2.2, "idle"), (2.2, 10.4, "waiting"), (10.4, DURATION, "idle")],
]

STRATEGIES = [
    (0, 2.6, "unified"),
    (2.6, 5.0, "auto_agent"),
    (5.0, 7.4, "manual_agent"),
    (7.4, 10.2, "auto_session"),
    (10.2, DURATION, "unified"),
]

AUTO_AGENT_ROUTE = {0: 0, 1: 1, 2: 2}
MANUAL_AGENT_ROUTE = {1: 0, 0: 1, 2: 2}
AUTO_SESSION_ROUTE = {0: 0, 1: 1, 2: 2, 3: 2}

STATE_RANK = {"idle": 0, "working": 1, "waiting": 2, "error": 3}


def value_at(segments, t):
    for start, end, value in segments:
        if start <= t < end:
            return value
    return segments[-1][2]


def boundaries(*segment_lists):
    times = {0.0, DURATION}
    for segments in segment_lists:
        for start, end, _ in segments:
            times.add(start)
            times.add(end)
    return sorted(t for t in times if 0 <= t <= DURATION)


def derive(times, fn):
    """按给定时间点采样并合并成平坦区间，用于聚合后的派生状态。"""
    out = []
    for start, end in zip(times, times[1:]):
        value = fn((start + end) / 2)
        if out and out[-1][2] == value:
            out[-1] = (out[-1][0], end, value)
        else:
            out.append((start, end, value))
    return out


def aggregate(values):
    best = "idle"
    for value in values:
        if STATE_RANK[value] > STATE_RANK[best]:
            best = value
    return best


def assignment(strategy):
    if strategy == "unified":
        return [list(range(len(SESSIONS)))] * 3
    if strategy == "auto_session":
        return [[i for i, dev in AUTO_SESSION_ROUTE.items() if dev == n] for n in range(3)]
    route = MANUAL_AGENT_ROUTE if strategy == "manual_agent" else AUTO_AGENT_ROUTE
    return [[i for i, (agent, _) in enumerate(SESSIONS) if route[agent] == dev] for dev in range(3)]


def caption_for(device, strategy, assigned, label):
    if strategy == "unified":
        return label["global"]
    if strategy == "auto_session":
        if len(assigned) == 1:
            return label["session_labels"][assigned[0]]
        agents = sorted({SESSIONS[i][0] for i in assigned})
        return " + ".join(label["agents_list"][a] for a in agents) if agents else "—"
    agents = sorted({SESSIONS[i][0] for i in assigned})
    if not agents:
        return "—"
    text = " + ".join(label["agents_list"][a] for a in agents)
    if strategy == "manual_agent":
        text += f"（{label['manual_tag']}）"
    return text


def fmt_time(t: float) -> str:
    return f"{t / DURATION:.5f}".rstrip("0").rstrip(".") if t else "0"


def keyframes(segments, fn):
    """按区间起点生成离散关键帧（SMIL，calcMode=discrete）。"""
    keytimes = [fmt_time(s) for s, _, _ in segments]
    values = [fn(value) for _, _, value in segments]
    return keytimes, values


def color_anim(attr, segments, fn):
    keytimes, values = keyframes(segments, fn)
    return (f'<animate attributeName="{attr}" dur="{DURATION}s" repeatCount="indefinite" '
            f'calcMode="discrete" keyTimes="{";".join(keytimes)}" values="{";".join(values)}"/>')


def vis_anim(segments, predicate):
    """只在 predicate 成立的区间可见。"""
    return color_anim("opacity", segments, lambda value: "1" if predicate(value) else "0")


class SVG:
    def __init__(self):
        self.parts: list[str] = []

    def add(self, text):
        self.parts.append(text)

    def text(self, x, y, size, content, fill=TEXT, anchor="start", bold=False, opacity=1.0, anims=""):
        weight = ' font-weight="700"' if bold else ""
        return (f'<text x="{x:.1f}" y="{y + size * 0.78:.1f}" font-size="{size}" fill="{fill}" '
                f'text-anchor="{anchor}" opacity="{opacity}"{weight}>{escape(content)}{anims}</text>')

    def rect(self, x, y, w, h, r=0, fill="none", stroke="none", sw=1, opacity=1.0, anims=""):
        return (f'<rect x="{x:.1f}" y="{y:.1f}" width="{w:.1f}" height="{h:.1f}" rx="{r}" fill="{fill}" '
                f'stroke="{stroke}" stroke-width="{sw}" opacity="{opacity}">{anims}</rect>')

    def circle(self, cx, cy, r, fill, opacity=1.0, anims=""):
        return (f'<circle cx="{cx:.1f}" cy="{cy:.1f}" r="{r}" fill="{fill}" opacity="{opacity}">'
                f'{anims}</circle>')

    def path(self, d, stroke, width=1.6, dash="5 5", opacity=1.0):
        return (f'<path d="{d}" fill="none" stroke="{stroke}" stroke-width="{width}" '
                f'stroke-dasharray="{dash}" opacity="{opacity}"/>')

    def group(self, body, opacity=1.0, animations=""):
        return f'<g opacity="{opacity}">{animations}{body}</g>'


def image_data(path: Path, height: int) -> tuple[str, float]:
    img = Image.open(path).convert("RGBA")
    scale = height / img.height
    img = img.resize((max(1, round(img.width * scale)), height), Image.LANCZOS)
    buf = io.BytesIO()
    img.save(buf, format="PNG", optimize=True)
    data = base64.b64encode(buf.getvalue()).decode()
    return f"data:image/png;base64,{data}", img.width / 2


def bezier(p0, p1, p2, prefix):
    d = f"M {p0[0]:.1f} {p0[1]:.1f} Q {p1[0]:.1f} {p1[1]:.1f} {p2[0]:.1f} {p2[1]:.1f}"
    return d


def build(lang: str) -> str:
    label = LABELS[lang]
    svg = SVG()

    # ---- 时间轴推导 ----
    times = boundaries(*SESSION_STATES, STRATEGIES)
    agent_states = []
    for agent in range(3):
        idxs = [i for i, (a, _) in enumerate(SESSIONS) if a == agent]
        agent_states.append(derive(
            times,
            lambda t, idxs=idxs: aggregate([value_at(SESSION_STATES[i], t) for i in idxs]),
        ))

    strategy_at = lambda t: value_at(STRATEGIES, t)
    device_states, captions = [], []
    for dev in range(3):
        states = derive(times, lambda t, dev=dev: aggregate(
            [value_at(SESSION_STATES[i], t) for i in assignment(strategy_at(t))[dev]]
        ) if strategy_at(t) != "unified" else aggregate([value_at(s, t) for s in SESSION_STATES]))
        device_states.append(states)
        caps = derive(times, lambda t, dev=dev: caption_for(
            dev, strategy_at(t), assignment(strategy_at(t))[dev], label))
        captions.append(caps)

    aggregate_states = derive(times, lambda t: aggregate([value_at(s, t) for s in SESSION_STATES]))

    # ---- 静态底 ----
    svg.add(f'<rect width="{W}" height="{H}" fill="{BG}"/>')
    svg.add(svg.text(40, 24, 26, "AgentBeacon", TEXT, bold=True))
    svg.add(svg.text(40, 66, 12.5, label["subtitle"], MUTED))

    agent_box = (40, 104, 300, 528)
    hub_box = (330, 182, 586, 428)
    device_box = (610, 104, 920, 528)

    svg.add(svg.text(agent_box[0] + 14, agent_box[1] - 24, 10.5, label["agents"], MUTED, bold=True))
    svg.add(svg.text(device_box[0] + 14, device_box[1] - 24, 10.5, label["devices"], MUTED, bold=True))

    row_h, gap = 133, 12
    card_h, card_gap = 136, 8
    agent_anchor = [(agent_box[2], agent_box[1] + i * (row_h + gap) + row_h / 2) for i in range(3)]
    device_anchor = [(device_box[0], device_box[1] + i * (card_h + card_gap) + card_h / 2) for i in range(3)]

    # ---- 连线（静态虚线 + 端点状态点）----
    for i, (ax, ay) in enumerate(agent_anchor):
        svg.add(svg.path(bezier((ax + 2, ay), (ax + 24, ay), (hub_box[0] - 2, hub_box[1] + 60 + i * 26), "a"), "#CBD5E1"))
    for i, (dx, dy) in enumerate(device_anchor):
        svg.add(svg.path(bezier((hub_box[2] + 2, hub_box[1] + 70 + i * 34), (hub_box[2] + 24, dy), (dx - 2, dy), "d"), "#CBD5E1"))
        svg.add(svg.circle(
            dx, dy, 3, "#9CA3AF",
            anims=color_anim("fill", device_states[i],
                             lambda v: STATE_RGB[STATE_COLOR[v]]),
        ))

    # ---- Agent 卡片 ----
    for i, name in enumerate(label["agents_list"]):
        y = agent_box[1] + i * (row_h + gap)
        w = agent_box[2] - agent_box[0]
        svg.add(svg.rect(agent_box[0], y, w, row_h, 12, CARD, BORDER, 1.2))
        # 非空闲时的高亮描边
        svg.add(svg.rect(
            agent_box[0], y, w, row_h, 12, "none", GREEN, 1.8, 0,
            anims=vis_anim(agent_states[i], lambda v: v != "idle")
            + color_anim("stroke", agent_states[i], lambda v: STATE_RGB[STATE_COLOR[v]]),
        ))

        icon = (agent_box[0] + 14, y + row_h / 2 - 17, 34, 34)
        svg.add(svg.rect(
            *icon, 10, SOFT["green"],
            anims=color_anim("fill", agent_states[i], lambda v: SOFT[STATE_COLOR[v]]),
        ))
        svg.add(svg.text(
            icon[0] + 17, icon[1] + 7, 18, name[0].upper(), GREEN, "middle", True,
            anims=color_anim("fill", agent_states[i], lambda v: STATE_RGB[STATE_COLOR[v]]),
        ))

        svg.add(svg.text(agent_box[0] + 62, y + row_h / 2 - 17, 15, name, TEXT, bold=True))

        # 状态点
        dot_x, dot_y = agent_box[0] + 63.5, y + row_h / 2 + 8
        svg.add(svg.circle(
            dot_x, dot_y, 3.5, "#9CA3AF",
            anims=color_anim("fill", agent_states[i], lambda v: STATE_RGB[STATE_COLOR[v]]),
        ))

        # 状态文字（每个取值一个 text，用透明度切换）
        variants = []
        if i == 0:
            variants = [("idle", label["state"]["idle"]),
                        ("working", f'{label["state"]["working"]} · {label["sessions"].format(n=2)}')]
        else:
            variants = [(s, label["state"][s]) for s in ("idle", "waiting", "error", "working")]
        for state, content in variants:
            svg.add(svg.group(
                svg.text(dot_x + 10, dot_y - 4, 11, content, MUTED),
                opacity=0,
                animations=vis_anim(agent_states[i], lambda v, s=state: v == s),
            ))

    # ---- 中枢 ----
    hx0, hy0, hx1, hy1 = hub_box
    hcx = (hx0 + hx1) / 2
    svg.add(svg.rect(hx0, hy0, hx1 - hx0, hy1 - hy0, 16, CARD, BORDER, 1.2))
    svg.add(svg.text(hcx, hy0 + 11, 17, "AgentBeacon", TEXT, "middle", True))
    svg.add(svg.text(hcx, hy0 + 31, 10, label["hub_sub"], MUTED, "middle"))

    # 聚合状态药丸
    pill = (hcx - 62, hy0 + 52, 124, 32)
    svg.add(svg.rect(
        *pill, 16, SOFT["green"], GREEN, 1.4,
        anims=color_anim("fill", aggregate_states, lambda v: SOFT[STATE_COLOR[v]])
        + color_anim("stroke", aggregate_states, lambda v: STATE_RGB[STATE_COLOR[v]]),
    ))
    svg.add(svg.text(hcx, pill[1] + 3, 9, label["aggregate"], MUTED, "middle"))
    for state in ("idle", "working", "waiting", "error"):
        svg.add(svg.group(
            svg.text(hcx, pill[1] + 17, 12.5, label["state"][state], STATE_RGB[STATE_COLOR[state]], "middle", True),
            opacity=0,
            animations=vis_anim(aggregate_states, lambda v, s=state: v == s),
        ))

    # 策略 chips
    svg.add(svg.text(hx0 + 14, hy0 + 96, 10.5, label["strategies"], MUTED, bold=True))
    chip_y = hy0 + 110
    for key in ("unified", "auto_agent", "manual_agent", "auto_session"):
        svg.add(svg.rect(
            hx0 + 12, chip_y, hx1 - hx0 - 24, 25, 12, "#F7F9FB", BORDER, 1.4,
            anims=color_anim("fill", STRATEGIES, lambda v, k=key: ORANGE if v == k else "#F7F9FB")
            + color_anim("stroke", STRATEGIES, lambda v, k=key: ORANGE if v == k else BORDER),
        ))
        svg.add(svg.text(
            hcx, chip_y + 7, 11, label["strategy"][key][0], MUTED, "middle",
            anims=color_anim("fill", STRATEGIES, lambda v, k=key: "#FFFFFF" if v == k else MUTED),
        ))
        chip_y += 29

    # 策略说明
    for key in ("unified", "auto_agent", "manual_agent", "auto_session"):
        svg.add(svg.group(
            svg.text(hcx, hy1 + 14, 10.5, label["strategy"][key][1], MUTED, "middle"),
            opacity=0,
            animations=vis_anim(STRATEGIES, lambda v, k=key: v == k),
        ))

    # ---- 设备卡片 ----
    defs = []
    for color, rgb in STATE_RGB.items():
        defs.append(
            f'<radialGradient id="glow-{color}"><stop offset="0" stop-color="{rgb}" stop-opacity="0.75"/>'
            f'<stop offset="0.55" stop-color="{rgb}" stop-opacity="0.28"/>'
            f'<stop offset="1" stop-color="{rgb}" stop-opacity="0"/></radialGradient>'
        )

    for i, (file, name, height) in enumerate(label["devices_list"]):
        y = device_box[1] + i * (card_h + card_gap)
        w = device_box[2] - device_box[0]
        svg.add(svg.rect(device_box[0], y, w, card_h, 12, CARD, BORDER, 1.2))

        href, img_w = image_data(ASSETS / file, height * 2)
        img_x = device_box[0] + 12
        img_y = y + (card_h - height) / 2
        cx, cy = img_x + img_w / 2, y + card_h / 2

        # 光晕：每种灯效一层，按时间轴显示
        effect_segments = [(s, e, EFFECT_OF_STATE[v]) for s, e, v in device_states[i]]
        for effect, color in (("calm", "green"), ("busy", "orange"), ("attention", "amber"), ("alert", "red")):
            pulse = ""
            if effect == "busy":
                pulse = ('<animate attributeName="opacity" values="0.22;0.85;0.22" keyTimes="0;0.5;1" '
                         f'dur="2.4s" repeatCount="indefinite"/>')
            elif effect == "alert":
                pulse = ('<animate attributeName="opacity" values="0.9;0.12" keyTimes="0;0.55" '
                         'calcMode="discrete" dur="1s" repeatCount="indefinite"/>')
            glow = svg.circle(cx, cy, 56, f"url(#glow-{color})")
            svg.add(svg.group(
                f'<g opacity="0.9">{pulse}{glow}</g>',
                opacity=0,
                animations=vis_anim(effect_segments, lambda v, ef=effect: v == ef),
            ))

        svg.add(f'<image x="{img_x:.1f}" y="{img_y:.1f}" width="{img_w:.1f}" height="{height}" '
                f'preserveAspectRatio="xMidYMid meet" href="{href}"/>')

        text_x = img_x + img_w + 14
        svg.add(svg.text(text_x, y + card_h / 2 - 36, 15, name, TEXT, bold=True))

        for content in dict.fromkeys(v for _, _, v in captions[i]):
            svg.add(svg.group(
                svg.text(text_x, y + card_h / 2 - 15, 10.5, content, FAINT),
                opacity=0,
                animations=vis_anim(captions[i], lambda v, c=content: v == c),
            ))

        dot_x, dot_y = text_x + 4, y + card_h / 2 + 14
        svg.add(svg.circle(
            dot_x, dot_y, 4, "#9CA3AF",
            anims=color_anim("fill", effect_segments, lambda v: EFFECT_COLOR[v]),
        ))

        for effect in ("calm", "busy", "attention", "alert"):
            svg.add(svg.group(
                svg.text(dot_x + 13, dot_y - 5, 11.5, label["effect"][effect], MUTED),
                opacity=0,
                animations=vis_anim(effect_segments, lambda v, ef=effect: v == ef),
            ))

    svg.add(svg.text(40, 536, 10, label["footnote"], FAINT))

    body = "".join(svg.parts)
    defs_block = "".join(defs)
    title = f'AgentBeacon — {label["subtitle"]}'
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" width="{W}" height="{H}" '
        f'role="img" aria-label="{escape(title)}">'
        f'<title>{escape(title)}</title>'
        f'<defs>{defs_block}</defs>'
        f'<style>text{{font-family:{FONT_FAMILY};}}</style>'
        f'{body}</svg>'
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--lang", choices=sorted(LABELS), default="zh")
    parser.add_argument("--out", type=Path)
    args = parser.parse_args()
    out = args.out or DOCS_ASSETS / f"how-it-works-{args.lang}.svg"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(build(args.lang), encoding="utf-8")
    print(f"{out}  {out.stat().st_size / 1024:.0f} KB")


if __name__ == "__main__":
    main()
