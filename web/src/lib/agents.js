/** 把 agent 名称/别名映射到 `AgentLogo` 使用的 id。 */
export function agentLogoId(agent) {
  const value = String(agent ?? '').toLowerCase()
  if (value.includes('claude')) return 'claude-code'
  if (value.includes('codex')) return 'codex'
  if (value.includes('gemini')) return 'gemini'
  if (value.includes('qwen')) return 'qwen'
  if (value.includes('iflow')) return 'iflow'
  if (value.includes('qoder')) return 'qoder'
  if (value.includes('opencode')) return 'opencode'
  if (value.includes('workbuddy') || value.includes('work buddy')) return 'workbuddy'
  if (value.includes('codebuddy') || value.includes('code buddy')) return 'codebuddy'
  if (value.includes('copilot')) return 'github-copilot'
  if (value.includes('hermes')) return 'hermes'
  if (value.includes('zcode') || value.includes('z-code')) return 'zcode'
  if (value.includes('antigravity')) return 'antigravity'
  if (value.includes('grok')) return 'grok'
  if (value === 'pi') return 'pi'
  if (value.includes('dsh')) return 'dsh'
  return agent
}
