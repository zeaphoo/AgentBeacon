# Tasks

## 1. 领域模型与配置（agent-beacon-core）

- [ ] 1.1 给 `SessionView` 增加 `created_at_ms`；首次插入写入、重复上报保留；验证：创建时间保留与过期后重建
- [ ] 1.2 新增 `aggregate_of(sessions)`，复用 `error > waiting > working > idle`、同优先级最近更新、空集合 `none`
- [ ] 1.3 新增 `RoutingStrategy`（`unified` / `auto_agent` / `manual_agent` / `auto_session`）与 `RoutingConfig { strategy, manual: HashMap<String, Vec<String>> }`；验证：默认 `unified`、旧配置加载得到默认值
- [ ] 1.4 删除逐设备 `assignment` / `pins` / `device_modes` / `empty_fallback` 配置；`DeviceBinding` 只保留 `id`/`serial`/`port`/`profile`/`label`/`brightness`/`buzzer_on_alert`
- [ ] 1.5 配置校验：策略枚举、`manual` 设备 id 与 Agent 非空、同一 Agent 不能出现在多台灯、亮度 `0..=100`、绑定至少一个设备键、label 长度上限
- [ ] 1.6 单元测试：旧配置兼容、非法策略、重复 Agent、空 Agent/设备、roundtrip

## 2. 分配引擎（agent-beacon 服务层）

- [ ] 2.1 新增 `routing` 模块：由策略推导设备角色（unified→mirror，auto_*→auto，manual→manual）
- [ ] 2.2 实现 `unified`：所有设备 mirror；`by_device` 为空，`unassigned_agents` 为空
- [ ] 2.3 实现 `auto_agent`：活动 Agent 按名字排序；粘性 agent→device；新 Agent 选 Agent 数最少的在线灯，平手按 id；同一 Agent 的 session 全部落在同一台灯
- [ ] 2.4 实现 `auto_session`：活动 session 按 `(created_at_ms, agent, session)` 排序；粘性 session→device；新 session 选 session 数最少的在线灯，平手按 id
- [ ] 2.5 实现 `manual_agent`：从 `routing.manual` 构造 Agent→灯；只在设备列表内的灯生效；重复 Agent 由校验阻止；未映射 Agent 进入 `unassigned_agents`
- [ ] 2.6 策略切换时清空旧粘性分配；`POST /routing/rebuild` 清空并全量重算
- [ ] 2.7 在线宽限：`connected` + `last_seen_ms`；`MISSING_GRACE_SECS = 60`；短暂离线保留归属，超过 grace 的 auto 设备退出分配集合
- [ ] 2.8 孤儿迁移：超过 grace 时只迁移失去归属的 Agent/session，其他设备不动；设备恢复后重新加入，不搬旧归属

## 3. 每设备目标状态（agent-beacon-lights + 服务）

- [ ] 3.1 `DesiredState` 改为 `devices: HashMap<String, DeviceDesired>` + `default: DeviceDesired` + `forced_off`
- [ ] 3.2 设备 actor 读取 `for_device(id)` 的 effect/brightness/buzzer；退出关灯强制 off
- [ ] 3.3 `BeaconState::register_device` / `unregister_device` 刷新 desired；设备插入后首次下发即为当前策略结果
- [ ] 3.4 空灯使用 `mapping.none`（默认 off），不再有单独的空分配回退设置

## 4. 快照与 API（beacon-service）

- [ ] 4.1 `Snapshot` 新增 `routing: Vec<DeviceRouteView>` 与 `unassigned_agents`；`DeviceRouteView` 包含 device/label/assignment/agents/sessions/aggregate/auto_effect/effect/overridden/connected/last_seen_ms
- [ ] 4.2 `routing` 覆盖已配置或已发现的设备；包含离线设备与配置绑定
- [ ] 4.3 新增 `POST /api/v1/routing/rebuild`
- [ ] 4.4 `GET /api/v1/state` 返回 `config.routing`（strategy + manual）；旧客户端忽略新增字段
- [ ] 4.5 `GET /api/v1/devices` 增加 `label` 与 `last_seen_ms`；其他既有字段不变

## 5. 设置页多灯控制策略（web-console）

- [ ] 5.1 保持灯具页原样，不添加多灯策略/Agent 分配/session 控件
- [ ] 5.2 设置页新增「多灯控制策略」卡片：四个策略按钮 + 说明
- [ ] 5.3 只有 `manual_agent` 展开灯列表：在线/离线/配置已失效；添加/移除 Agent；同一 Agent 只能属于一台灯
- [ ] 5.4 离线/失效灯保留配置；提供「删除这台灯的配置」，同时移除 `manual` 条目与 `devices` 绑定
- [ ] 5.5 自动策略显示说明、未分配 Agent 与「重新分配」按钮
- [ ] 5.6 窄屏与键盘可达；保存失败保留输入

## 6. 测试与端到端

- [ ] 6.1 单元测试：四种策略、Agent/session 粘性、手动映射、重复 Agent、grace、空灯、旧配置
- [ ] 6.2 API 测试：`routing`/`unassigned_agents`/`config.routing`、rebuild、旧客户端字段
- [ ] 6.3 前端测试：四策略切换、手动添加/移除 Agent、离线灯保留与删除、自动策略重新分配
- [ ] 6.4 e2e：3 台 mock 灯 + 1 Agent 3 session，验证 `auto_session` 1/1/1、`auto_agent` 全部在同一台、`manual_agent` 离线灯保留与删除
- [ ] 6.5 手工 smoke：真实设备切换四种策略，拔插一盏灯验证 grace 与孤儿迁移，删除失效灯配置

## 7. 文档与发布

- [ ] 7.1 更新 README / development 文档：四种策略、`routing.strategy`/`routing.manual`、rebuild 接口
- [ ] 7.2 更新 CHANGELOG：多灯控制策略、设置页、离线灯保留/删除
- [ ] 7.3 更新协议/硬件清单中与多灯相关的说明（如有）
