# 品牌标识素材

控制台「Agent 集成」页用的品牌标识。全部随 Web 资源打包，离线可用，不从 CDN 加载。

| 文件 | 来源 |
| --- | --- |
| `qoder.png` | https://qoder.com 站点图标（412×412 PNG） |
| `iflow.png` | https://iflow.cn 站点图标（72×72 PNG） |
| `codebuddy.svg` | https://www.codebuddy.ai 站点图标 |
| `workbuddy.svg` | https://www.workbuddy.ai 站点图标 |
| `hermes.png` | https://hermes-agent.nousresearch.com 应用图标（128×128 PNG，由 1024×1024 缩放） |
| `zcode.png` | https://zcode.z.ai 站点图标（192×192 PNG） |

其余 agent 为内联单色/品牌色 SVG，路径取自 [Simple Icons](https://simpleicons.org)（CC0）与各自官网图标：
Pi（pi.dev）、OpenCode、DeepSeek、Claude、OpenAI（Codex CLI）、GitHub Copilot、Google Gemini、Qwen。

新增 agent 时：优先放一份方形图标到本目录，在 `web/src/components/AgentLogo.jsx` 的 `IMAGES` 里登记；
单色商标也可以直接写进 `MARKS`。未登记的集成会自动回落为通用插头图标。
