# Spec Delta

## Purpose

在设置页提供独立的多灯控制策略，不改变灯具页原有的手动灯效与设备型号管理；不提供 session 手动绑定。

## MODIFIED Requirements

### Requirement: 状态总览

控制台首页 SHALL 显示当前全局聚合状态、当前全局灯效、活动会话列表（agent、session、状态、消息、最近更新时间）、每台灯的独立状态卡片与服务运行信息。

- 顶部「当前聚合状态」MUST 表示**所有会话**的聚合，与当前多灯策略无关。
- 每台灯一张状态卡片，展示该灯自己的范围聚合状态、当前灯效、正在显示的 Agent、连接状态与设备信息；视觉结构 SHALL 参考顶部聚合卡片（状态图标、状态名称、灯效 badge、状态色调）。
- 无论 `unified` / `auto_agent` / `manual_agent` / `auto_session`，每台灯都 MUST 以自己的 `routing[].aggregate` / `routing[].effect` 显示，不得复用全局聚合。
- 离线灯 SHALL 显示为未连接，并显示其最近一次分配/回退状态。

多灯策略的详细配置 MUST 放在设置页，首页只展示全局状态与每台灯的状态。

#### Scenario: 有活动会话

- **WHEN** 两个 agent 分别处于 `working` 与 `waiting`
- **THEN** 首页突出显示全局聚合状态 `waiting`，并列出两条会话及其状态与更新时间

#### Scenario: 每台灯独立状态

- **WHEN** 策略为 `auto_agent`，左灯分到 Claude（`working`），右灯分到 Codex（`error`）
- **THEN** 顶部全局聚合显示 `error`，左灯卡片显示「运行中」，右灯卡片显示「错误」，并分别显示 Claude / Codex

#### Scenario: 统一显示

- **WHEN** 策略为 `unified`
- **THEN** 顶部与每台灯卡片显示同一个全局聚合状态与灯效

#### Scenario: 无设备

- **WHEN** 没有任何受支持设备连接
- **THEN** 首页显示「未发现设备」提示及排查建议，页面其余信息仍可查看

### Requirement: 灯具页

控制台「灯具」页面 SHALL 保持原有职责：手动灯光覆盖控件、设备列表（标签、profile、连接状态、当前灯效）、按设备覆盖 profile 与重新扫描，以及「支持的型号」对话框。该页面的配置修改 SHALL 与其他页面共用同一份配置并即时保存。

灯具页 MUST NOT 承载多灯策略、Agent 分配或 session 分配控件。

#### Scenario: 覆盖设备型号

- **WHEN** 用户在灯具页把某个已连接设备的 profile 改为 `mlight-3led` 并保存
- **THEN** 该设备的绑定写入配置，页面显示保存成功，设备按新 profile 的能力展示

#### Scenario: 查看支持的型号

- **WHEN** 用户点击「支持的型号」
- **THEN** 弹出对话框列出型号、说明与能力，列表可纵向滚动，关闭后回到设备列表

### Requirement: 配置编辑

控制台 SHALL 提供设置页，用于编辑状态到灯效的映射、手动覆盖默认时长、退出时是否关灯、全局亮度、全局报警蜂鸣、mock 设备开关，以及独立的多灯控制策略。设备标签与 profile SHALL 在「灯具」页维护。

保存成功 SHALL 给出成功反馈并即时生效；保存失败 MUST 显示服务返回的错误信息且不丢失用户输入。

#### Scenario: 修改映射

- **WHEN** 用户把 `idle` 的映射改为 `off` 并保存
- **THEN** 页面提示保存成功，对应聚合状态为 `idle` 的设备熄灭

#### Scenario: 保存失败

- **WHEN** 服务返回 400 配置校验错误
- **THEN** 设置页显示错误原因，已填写的表单内容保持不变

## ADDED Requirements

### Requirement: 多灯控制策略

设置页 SHALL 提供独立的「多灯控制策略」卡片，以单选方式提供四种策略：

- 统一显示：所有灯显示同一个全局状态；
- 按 Agent 自动分配：每个 Agent 自动分到一台灯；
- 按 Agent 手动分配：手动把 Agent 分到灯；
- 按 Session 自动分配：同一个 Agent 的多个 session 自动分散到不同灯。

每种策略 SHALL 显示一句简短说明；选中后 SHALL 显示该策略的详细说明。策略是全局单选，MUST NOT 提供逐设备的模式选择。

#### Scenario: 切换策略

- **WHEN** 用户选择「按 Session 自动分配」
- **THEN** 配置写入 `routing.strategy = auto_session`，页面显示该策略说明

#### Scenario: 统一显示

- **WHEN** 用户选择「统一显示」
- **THEN** 页面不再显示 Agent 映射控件，所有灯显示同一个全局状态

### Requirement: 手动分配灯列表

只有选择「按 Agent 手动分配」时，设置页 SHALL 展开详细配置：

- 列出所有灯：已检测到的灯、配置中的离线灯、`routing.manual` 中保留但当前不存在的灯；
- 每台灯显示名称、型号、端口/设备 id、在线/离线/配置已失效状态；
- Agent 选择与已分配 Agent 放在设备信息右侧；已分配的 Agent 以可移除的 chip 展示；
- Agent chip 与选择项 SHALL 显示品牌图标；未收录品牌回落到通用图标；
- 每台灯提供「添加 Agent」选择器，一个 Agent MUST 只能属于一台灯；
- 未检测到的灯 SHALL 提供「删除这台灯的配置」；删除 MUST 移除该灯的 `manual` 条目和 `devices` 绑定；
- 没有 Agent 的灯 SHALL 显示「还没有分配 Agent」。

#### Scenario: 一台灯多个 Agent

- **WHEN** 用户把 Claude 和 Codex 都添加到「左灯」
- **THEN** 配置写入 `routing.manual["左灯的设备 id"] = ["claude","codex"]`

### Requirement: Agent 选项

「添加 Agent」的选项 SHALL 包含：

- 当前活动 session 的 Agent；
- 已手动映射的 Agent；
- 集成检测到的所有 Agent（即使尚未上报 session）。

只有在完全没有任何 Agent 时，选择器才 SHALL 禁用并提示先安装/启动 Agent。

#### Scenario: Agent 尚未上报 session

- **WHEN** 系统检测到 Claude Code，但它还没有上报任何 session
- **THEN** 手动分配的「添加 Agent」列表仍然包含 Claude Code

#### Scenario: 没有任何 Agent

- **WHEN** 既没有活动 session，也没有检测到任何集成
- **THEN** 选择器禁用，并提示先在「Agent 集成」页面安装或启动 Agent

#### Scenario: 移动 Agent

- **WHEN** 用户把 Claude 从「左灯」添加到「右灯」
- **THEN** Claude 从左灯的列表移除，只保留在右灯

#### Scenario: 保留离线灯

- **WHEN** 一台已经分配 Claude 的灯被拔掉
- **THEN** 设置页仍显示该灯与 Claude，并标记为未连接；重新插上后继续生效

#### Scenario: 删除失效灯

- **WHEN** 用户点击一台未检测到灯的「删除这台灯的配置」
- **THEN** 该灯从列表消失，其 Agent 变为未分配

### Requirement: 自动策略提示

选择 `auto_agent` 或 `auto_session` 时，设置页 SHALL 显示策略说明、当前未分配 Agent（若非空）与「重新分配」按钮；点击按钮 SHALL 调用 `POST /api/v1/routing/rebuild` 并刷新界面。

#### Scenario: 重新分配

- **WHEN** 用户点击「重新分配」
- **THEN** 前端调用重建接口，完成后按最新快照更新提示

#### Scenario: 未分配 Agent

- **WHEN** 快照的 `unassigned_agents` 非空
- **THEN** 设置页显示未分配 Agent 的名称

### Requirement: 不提供 session 手动绑定

控制台 MUST NOT 提供 session 级选择器、拖拽 session 或任何按 session 绑定设备的操作。session 只能由自动策略分配，或通过 Agent 手动映射间接决定。

#### Scenario: 没有 session 选择器

- **WHEN** 用户查看多灯控制策略
- **THEN** 只能选择四种策略或在手动模式里分配 Agent，不能为单个 session 选择灯

### Requirement: 响应式与键盘可达

新增的策略卡片与手动分配控件 SHALL 在 375 像素宽度下无横向滚动；关键操作 MUST 可通过键盘完成，不依赖悬停。

#### Scenario: 窄屏

- **WHEN** 在 375 像素宽度的浏览器中打开设置页
- **THEN** 四个策略按钮纵向或两列排列，手动灯列表可正常操作

#### Scenario: 键盘操作

- **WHEN** 用户仅使用 Tab/Enter
- **THEN** 可以切换策略、添加/移除 Agent 并保存
