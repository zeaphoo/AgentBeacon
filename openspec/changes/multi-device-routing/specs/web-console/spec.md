# Spec Delta

## Purpose

让控制台能开启多灯自动分配、把某个 agent 固定到某台灯，并解释每台灯为什么显示当前颜色；不提供 session 绑定，保持全局状态卡片与单灯用户体验不变。

## MODIFIED Requirements

### Requirement: 状态总览

控制台首页 SHALL 显示当前全局聚合状态、当前全局灯效、活动会话列表（agent、session、状态、消息、最近更新时间）、设备列表（标签、端口、profile、分配方式、分到的 agent、session 数、连接状态、当前灯效）与服务运行信息。无活动会话或无设备时 SHALL 显示明确的空状态说明。

全局聚合卡片与设备列表 MUST 使用不同措辞，明确前者是「所有 session 的聚合」，后者是「该设备分到的 session 的聚合」，避免多设备分配时用户误以为两者应一致。

#### Scenario: 有活动会话

- **WHEN** 两个 agent 分别处于 `working` 与 `waiting`
- **THEN** 首页突出显示全局聚合状态 `waiting`，并列出两条会话及其状态与更新时间

#### Scenario: 多设备不同效果

- **WHEN** 设备 A 分到 Claude 的 `working` session，设备 B 分到 Codex 的 `error` session
- **THEN** 顶部全局状态显示 `error`，设备列表分别显示 A 的 `busy` 与 B 的 `alert`，并标注各自分到的 agent

#### Scenario: 无设备

- **WHEN** 没有任何受支持设备连接
- **THEN** 首页显示「未发现设备」提示及排查建议，页面其余信息仍可查看

### Requirement: 灯具页

控制台 SHALL 提供「灯具」页面，集中呈现全部灯具相关信息与配置：手动灯光覆盖控件、设备列表（含标签、profile、分配方式、在线/离线、亮度/蜂鸣覆盖、重新扫描）、 **Agent 分配面板**，以及入口按钮打开的「支持的型号」对话框。该页面的配置修改 SHALL 与其他页面共用同一份配置并即时保存。

设备列表 MUST 合并已发现设备与配置中的绑定：已配置但当前未检测到的绑定显示为「未连接」，不提供删除/停用按钮。页面 MUST NOT 提供删除设备或停用设备的操作；设备只有检测到和未检测到两种状态。把 `assignment` 改为 `mirror` 可以让设备退出自动分配，或直接拔掉设备。对只能按端口绑定的设备，页面 SHALL 提示「重插后端口可能变化」。短暂离线时页面 SHALL 说明归属会保留，超过 `MISSING_GRACE_SECS` 后其 session 会自动迁移。

#### Scenario: 覆盖设备型号

- **WHEN** 用户在灯具页把某个已连接设备的 profile 改为 `mlight-3led` 并保存
- **THEN** 该设备的绑定写入配置，页面显示保存成功，设备按新 profile 的能力展示

#### Scenario: 开启自动分配

- **WHEN** 用户把设备 A 的分配方式从「镜像全局」改为「自动分配」并保存
- **THEN** 配置写入 `assignment: auto`，A 开始参与 session 自动分配

#### Scenario: 创建手动专用灯

- **WHEN** 用户把设备 B 的分配方式改为「仅手动固定」
- **THEN** 配置写入 `assignment: manual`，B 只接收 pin 到它的 agent，不接收自动 session

#### Scenario: 没有删除或停用操作

- **WHEN** 用户查看灯具页的设备行
- **THEN** 只能编辑标签、profile、分配方式、亮度/蜂鸣，不能删除或停用设备

#### Scenario: 离线设备提示

- **WHEN** 一台设备未被检测到
- **THEN** 页面显示「未连接」与最后连接时间，并说明短暂离线保留归属、超过 grace 后自动迁移

#### Scenario: 查看支持的型号

- **WHEN** 用户点击「支持的型号」
- **THEN** 弹出对话框列出型号、说明与能力，列表可纵向滚动，关闭后回到设备列表

### Requirement: 配置编辑

控制台 SHALL 提供设置页，用于编辑状态到灯效的映射、手动覆盖默认时长、退出时是否关灯、全局亮度、全局报警蜂鸣、mock 设备开关，以及 `routing.empty_fallback`。设备标签、profile、分配方式、每设备亮度/蜂鸣与 agent 固定 SHALL 在「灯具」页维护。

保存成功 SHALL 给出成功反馈并即时生效；保存失败 MUST 显示服务返回的错误信息且不丢失用户输入。

#### Scenario: 修改映射

- **WHEN** 用户把 `idle` 的映射改为 `off` 并保存
- **THEN** 页面提示保存成功，对应聚合状态为 `idle` 的设备熄灭

#### Scenario: 修改空分配回退

- **WHEN** 用户把「空分配时」改为「跟随全局聚合」并保存
- **THEN** 没有分到 session 的设备改为显示全局聚合灯效

#### Scenario: 保存失败

- **WHEN** 服务返回 400 配置校验错误
- **THEN** 设置页显示错误原因，已填写的表单内容保持不变

## ADDED Requirements

### Requirement: Agent 分配面板

控制台 SHALL 在灯具页提供 Agent 分配面板，列出当前活动 agent，显示每个 agent 的 session 数与当前落点，并为每个 agent 提供「自动 / 固定到设备」选择器。选择「自动」时 session 由算法分配；选择 `auto` 或 `manual` 设备时写入 `routing.pins`。面板 MUST NOT 提供 session 级选择器或任何按 session 绑定设备的操作。

#### Scenario: 固定 agent

- **WHEN** 用户把 Claude 的分配从「自动」改为「固定到 左灯」
- **THEN** 配置写入 `routing.pins["claude-code"] = "serial:…"`，Claude 的全部 session 落到左灯

#### Scenario: 取消固定

- **WHEN** 用户把 Claude 改回「自动」
- **THEN** 配置删除对应 pin，Claude 的 session 进入自动池，按算法分散到各 auto 设备

#### Scenario: 没有 session 选择器

- **WHEN** 用户查看 Agent 分配面板
- **THEN** 只能操作 agent，不能为单个 session 选择灯

### Requirement: Agent 固定行为与提示

固定 agent 时，控制台 SHALL 提示该 agent 的全部 session 会留在同一台灯。固定到 `auto` 设备时 SHALL 提示该灯仍可能接收自动 session；固定到 `manual` 设备时 SHALL 说明该灯只显示固定 agent。没有 `auto`/`manual` 设备时，面板 SHALL 提示先开启相应模式。

#### Scenario: 固定到手动专用灯

- **WHEN** 用户把 Claude 固定到一台 `assignment: manual` 的设备
- **THEN** 该设备只显示 Claude 的 session，其他 agent 的 session 不会进入这台设备

#### Scenario: 手动设备没有固定

- **WHEN** 一台 `manual` 设备没有任何 pin
- **THEN** 页面提示该设备只接收固定 agent，并显示空分配回退

### Requirement: 分配预览与重新分配

控制台 SHALL 在灯具页展示分配预览：每台设备分到的 agent、session 数、聚合状态与当前灯效；空分配设备 SHALL 显示回退方式。页面 SHALL 提供「重新分配」操作，调用 `POST /api/v1/routing/rebuild`，并在完成后按最新快照更新预览。

当 `unassigned_agents` 或 `unresolved_pins` 非空时，页面 SHALL 显示对应提示：未被任何灯分配的 agent，以及固定目标不可用的 pin。

#### Scenario: 查看分配预览

- **WHEN** 3 台 auto 灯、1 个 agent 3 个 session
- **THEN** 预览显示每台灯各 1 个 session，并显示各自的聚合状态

#### Scenario: 新增灯后重新分配

- **WHEN** 用户新增一台 auto 灯并点击「重新分配」
- **THEN** 前端调用重建接口，预览按新设备集合更新

#### Scenario: 固定目标不可用

- **WHEN** 快照的 `unresolved_pins` 非空
- **THEN** 页面显示「固定目标不可用」及 agent/设备信息，并提供改为「自动」的操作

### Requirement: 多设备分配的可解释性

控制台 SHALL 在设备上同时展示「分配方式」与「当前灯效」，并在手动覆盖生效时标注「已覆盖」及剩余时间。全局覆盖与单设备覆盖 MUST 使用不同措辞，避免用户误以为只影响一台灯。

页面 SHALL 说明自动分配规则：session 会优先分到当前 session 最少的灯，已有 session 不会被新 session 挤走；同一 agent 的多个 session 可以分散到不同的灯。设备长时间离线（超过 grace）时只迁移原本在该设备上的 session，其他灯不动；设备恢复在线不会自动搬走旧 session，需要立即平衡时点击「重新分配」。

#### Scenario: 单设备覆盖提示

- **WHEN** 用户只对设备 A 应用 `alert`
- **THEN** 设备 A 显示覆盖标记与倒计时，其他设备不显示覆盖标记

#### Scenario: 全局覆盖提示

- **WHEN** 用户对全部设备应用 `alert`
- **THEN** 所有设备显示覆盖标记，页面明确说明这是全局覆盖
