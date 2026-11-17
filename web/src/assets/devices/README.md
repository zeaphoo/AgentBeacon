# 设备配图素材

控制台设备卡片、设备列表与「支持的型号」弹窗使用的型号渲染图。全部随 Web 资源打包，
离线可用，不从 CDN 加载；后端不下发图片 URL。

| 文件 | 对应 profile | 说明 |
| --- | --- | --- |
| `agentcore-light.png` | `agentcore-light` | 三色灯塔状态灯 |
| `agentcore-halo.png` | `agentcore-halo`、`mlight-3led` | 白色立方桌面灯；`mlight-3led` 外观相同，共用同一张图 |

素材由项目维护者提供。替换或新增时注意：

- 保留透明通道（RGBA），不要白底/黑底；
- 先按 alpha 包围盒裁掉多余空白，再统一留 6% 透明边距；
- 长边导出 256px 即可（界面最大显示 64px，3 倍屏足够）；
- 单张压到 ≤45KB；
- 非方形素材可以直接用：组件用固定槽位 + `object-contain`，缩放补偿系数在
  `web/src/components/DeviceImage.jsx` 的 `DEVICE_IMAGES` 里维护。

未收录的 profile 会自动回落到通用 USB 图标。
