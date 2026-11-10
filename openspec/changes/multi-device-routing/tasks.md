# Tasks

## 1. 领域模型与配置（agent-beacon-core）

- [ ] 1.1 给 `SessionView` 增加 `created_at_ms`；`SessionRegistry::apply` 在首次插入时写入、重复上报时保留；验证：`cargo test -p agent-beacon-core session` 覆盖创建时间保留与过期后重建
- [ ] 1.2 新增 `aggregate_of(sessions)`（或等价 API），复用 `error > waiting > working > idle`、同优先级最近更新、空集合 `none`；让 `aggregate_at` 复用它；验证：单元测试覆盖单集合、多优先级、空集合
- [ ] 1.3 新增 `DeviceAssignment`（`mirror`/`auto`/`manual`，默认 `mirror`）与 `EmptyAssignmentFallback`（`none`/`global`/`off`）；验证：serde roundtrip 与默认值
- [ ] 1.4 扩展 `RoutingConfig`：`empty_fallback`（默认 `none`）与 `pins: HashMap<String,String>`（默认空）；`Config` 增加 `routing`；验证：空配置、旧配置加载得到默认值
- [ ] 1.5 扩展 `DeviceBinding`：`id`、`label`、`assignment`（默认 `mirror`）、`brightness`、`buzzer_on_alert`；验证：旧配置（只有 `serial`/`port`/`profile`）加载后新字段取默认值；不引入 `enabled`
- [ ] 1.6 扩展 `Config::validate()`：`assignment` 枚举、pin 的 agent/设备 id 非空、亮度 `0..=100`、绑定至少一个 `id`/`serial`/`port`、`label` 长度上限；验证：非法配置返回对应 `ConfigError`，合法旧配置通过

## 2. 自动分配引擎（agent-beacon 服务层）

- [ ] 2.1 新增 `assignment` 模块：维护 `session -> device` 的粘性分配表与孤儿迁移逻辑；验证：设备长时间离线只迁移原设备上的 session、设备恢复在线不搬旧 session、session 增删不动其他 session、显式重建才全量重算
- [ ] 2.2 实现单次分配：活动 session 按 `(created_at_ms, agent, session)` 排序；固定 agent 先落到 pin 目标；其余 session 选当前自动分配集合中 session 数最少的 `auto` 设备，平手按设备 id 升序；已有有效分配保持不动；验证：3 session/3 灯 → 1/1/1，4/3 → 2/1/1，新增 session 不移动已有
- [ ] 2.3 实现 pin 解析：目标必须是 `auto` 或 `manual` 设备；目标不存在、超过 grace 未检测到或 `mirror` 时进入未解析列表，agent 的 session 回到自动池；验证：固定、取消固定、长时间离线、固定到 manual 四种情况
- [ ] 2.4 实现设备集合：参与自动分配的是 `connected: true` 的 `auto` 设备，以及离线但在 `MISSING_GRACE_SECS` 内的 `auto` 设备；从未连接和超过 grace 的 auto 设备不进入集合；`manual` 设备只接收 pin；验证：短暂离线保留归属、超过 grace 后迁移、从未连接的设备不参与、manual 不接收自动 session
- [ ] 2.5 在 `DeviceInfo` 增加 `last_seen_ms`，actor 成功打开连接时更新；定义 `MISSING_GRACE_SECS`（默认 60 秒）；验证：断开后保留最后时间、重连后更新时间、按时间判断 grace
- [ ] 2.6 实现显式重建接口的内部方法（清空分配表并全量重算），供 `POST /routing/rebuild` 调用；配置变化走孤儿迁移而不是全量重建；验证：设备恢复在线后旧 session 不动，调用重建后才重新平衡
- [ ] 2.7 计算每设备的 session 集合、agent 集合、`pinned_agents`、聚合状态、`auto_effect`、`fallback`；mirror 设备使用全部 session，manual 设备只使用 pin 到的 session；验证：多 agent 聚合、空分配三种回退、manual 专用灯
- [ ] 2.8 实现孤儿迁移：设备超过 grace 未检测到、`auto`→`mirror`/`manual`、pin 变化时只迁移失去有效归属的 session，其他 session 保持原设备；验证：一台灯离线超过 grace 只迁移它原来的 session，auto→manual 保留 pin 到的 session，设备恢复不搬旧 session

## 3. 每设备目标状态（agent-beacon-lights + 服务）

- [ ] 3.1 将 `DesiredState` 改为 `devices: HashMap<String, DeviceDesired>` + `default: DeviceDesired` + `forced_off: bool`，新增 `for_device(id)`；验证：单元测试覆盖命中、未命中、`forced_off`
- [ ] 3.2 设备 actor 改为读取 `for_device(id)` 的 `effect` / `brightness` / `buzzer_on_alert`，亮度比较与重放逻辑保持；验证：actor 测试覆盖两台设备不同灯效、每设备亮度、`forced_off` 下发 off
- [ ] 3.3 `BeaconState` 计算每设备 `DeviceDesired`：分配结果聚合 + 全局映射 + 覆盖 + 空分配回退 + 每设备亮度/蜂鸣覆盖；新设备注册时立即刷新；`clear_sessions` 同时清空分配表；验证：`state` 测试断言两台设备的 desired 不同、覆盖到期回到分配结果
- [ ] 3.4 新增 `BeaconState::register_device()` / `remove_device()`，worker 在 upsert/remove 时调用并刷新 desired；验证：设备插入后首次下发的效果已是其分配结果，不会先闪全局效果

## 4. 快照与 API（beacon-service）

- [ ] 4.1 扩展 `Snapshot`：新增 `routing: Vec<DeviceRouteView>`、`unassigned_agents: Vec<String>`、`unresolved_pins: Vec<PinView>`；`DeviceRouteView` 包含 `device`、`label`、`assignment`、`agents`、`pinned_agents`、`sessions`、`aggregate`、`auto_effect`、`effect`、`overridden`、`fallback`、`connected`、`last_seen_ms`；验证：`GET /api/v1/state` 返回新字段，SSE 快照一致
- [ ] 4.2 `routing` 覆盖每台已配置或已发现的设备，包括当前未检测到的绑定；同一设备 id 只出现一次；验证：配置一台未连接的 auto 设备，快照仍包含其路由且 `connected: false`、`last_seen_ms` 可空
- [ ] 4.3 新增 `POST /api/v1/routing/rebuild`：清空粘性分配并重算，不修改配置；验证：API 测试调用后 `routing` 更新、`GET /config` 不变
- [ ] 4.4 保持 `GET /api/v1/devices` 契约不变，补充 `label`；验证：API 测试断言既有字段仍在、`label` 可空
- [ ] 4.5 确认 `PUT /api/v1/config` 保存 `assignment`/pins 后只迁移受影响的 session（长时间离线、`auto`→`manual`/`mirror`、pin 变化），其他 session 保持不动并广播；验证：API 测试让一台 auto 设备离线超过 grace 后只有该设备的 session 迁移，其他设备归属不变
- [ ] 4.6 检查旧配置与旧客户端兼容：默认 `mirror`、`created_at_ms` 为增量字段、`version` 仍为 `1`；验证：用变更前的配置样例启动服务，设备行为不变

## 5. Web 控制台（web-console）

- [ ] 5.1 更新 API/store：消费 `routing`、`unassigned_agents`、`unresolved_pins`、`created_at_ms`，保存配置时保留新字段；验证：Vitest 断言快照合并与配置 roundtrip
- [ ] 5.2 状态页：设备行显示标签、分配方式、在线/离线、最后连接时间、分到的 agent、session 数、当前灯效与覆盖标记，区分全局聚合与设备效果；验证：Testing Library 用多设备快照断言渲染
- [ ] 5.3 灯具页：每台设备增加标签、分配方式（镜像/自动/仅手动固定）、亮度/蜂鸣覆盖；合并已发现设备与配置绑定，显示「未连接」；不提供删除/停用按钮；验证：交互测试断言保存请求体中的 `label`、`assignment`，且页面不存在 `enabled` 或删除/停用操作
- [ ] 5.4 Agent 分配面板：列出活动 agent、session 数、当前落点；提供「自动 / 固定到设备」选择器，目标包括 auto 与 manual 设备；固定写入 `routing.pins`，取消固定删除；没有 auto/manual 设备时提示；验证：交互测试断言 pin 请求体，且页面不存在 session 选择器
- [ ] 5.5 分配预览：每台灯显示分到的 agent/session、聚合状态、灯效、空分配回退；提供「重新分配」按钮调用重建接口；验证：Testing Library 断言预览内容与重建请求
- [ ] 5.6 异常提示：`unassigned_agents` 与 `unresolved_pins` 非空时显示提示与操作入口；验证：Testing Library 断言两类提示
- [ ] 5.7 设置页：增加 `routing.empty_fallback` 控件；验证：保存请求体包含新字段，失败时保留输入
- [ ] 5.8 响应式与键盘可达：新控件在 375px 下无横向滚动，Tab/Enter 可完成分配方式与 pin 编辑；验证：现有布局测试扩展

## 6. 测试与端到端

- [ ] 6.1 单元测试补齐：创建时间、集合聚合、配置校验、分配算法、pin、孤儿迁移、离线 grace、`last_seen_ms`、回退、每设备 desired；验证：`cargo test --workspace` 通过
- [ ] 6.2 e2e：3 台 mock 灯 + 1 个 agent 3 个 session，断言三台灯各 1 个 session、mock 命令不同；pin 该 agent 到灯 A 后，A 聚合 3 个 session、B/C 回退 off；取消 pin 后恢复 1/1/1；验证：`cargo test -p agent-beacon e2e`
- [ ] 6.3 e2e：4 个 session/3 台灯 → 2/1/1；新增第 5 个 session 不移动已有；删除一个 session 只移除自己；一台灯短暂离线（grace 内）归属不变，超过 grace 后只有该灯上的 session 迁移，其他 session 不动；设备恢复后重新加入且不搬旧 session；验证：分配引擎测试固定时钟断言
- [ ] 6.4 前端测试：状态页分配视图、灯具页 Agent 面板（无 session 选择器）、分配预览、异常提示、设置页回退；验证：`pnpm --dir web test` 通过
- [ ] 6.5 手工 smoke：真实设备 A（serial）设为 auto、B（BLE）设为 manual 并 pin 一个 agent，1 个 agent 多 session 时观察 auto 灯分散与 manual 灯专用；拔掉一盏灯后在 grace 内验证其他灯归属不变，超过 grace 后只有它原来的 session 迁移；重新插上验证设备重新加入；验证：在 `docs/hardware-checklist.md` 记录结果

## 7. 文档与发布

- [ ] 7.1 更新 `README.md` / `README.zh-CN.md`：多灯自动分配、agent 固定、session 不手动绑定、默认镜像行为；验证：照文档配置两台灯可复现
- [ ] 7.2 更新 `docs/development.md` / `docs/development.zh-CN.md`：状态快照新增字段、配置字段、自动分配算法与重建接口；验证：API 表格与示例与实现一致
- [ ] 7.3 更新 `CHANGELOG.md` / `CHANGELOG.zh-CN.md`：记录自动分配、agent 固定、每设备目标状态与回退设置；验证：条目与变更范围一致
