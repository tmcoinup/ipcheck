# IPCheck 架构说明

## 分层

```
┌─────────────────────────────────────────┐
│  app (Iced Application)                 │
│  view / Message / Command / Subscription │
└─────────────────┬───────────────────────┘
                  │
┌─────────────────▼───────────────────────┐
│  service::IpCheckService<R>             │
│  编排：导入、快照、探针、风控、持久化    │
└─────────────────┬───────────────────────┘
                  │
┌─────────────────▼───────────────────────┐
│  repository::AppRepository (trait)      │
│  SqliteRepository                       │
└─────────────────────────────────────────┘
```

- **Handler 不写业务**：`Message` 在 `update` 中调用 `IpCheckService` 或 `Command::perform` 异步任务。
- **可替换存储**：业务依赖 `AppRepository`，便于测试与替换实现。
- **领域模型**：`domain::models` 定义 `ProxyEntry`、`CheckResult` 等，与 UI、HTTP DTO 解耦方向由现有代码体现。

## 服务子模块（`service/ip_service/`）

| 模块 | 职责 |
|------|------|
| `proxy_parse` | 多格式代理行解析 |
| `ip_probe` | 经 SOCKS 探测出口 IP |
| `http_client` / `http_headers` | 直连与代理 HTTP 客户端 |
| `baidu_api` | 百度智能云 IP 画像/风控接口 |
| `error` | `ServiceError` |

## 异步与 UI

- 使用 **Tokio** 与 `Command::perform` 执行异步风控、导入等，结果以 `Message` 回传。
- 窗口操作（如启动最大化）在收到 `WindowOpened` 后执行 `iced::window::maximize`。

## 扩展点

- 新接口：在 `service` 增加模块，经 `IpCheckService` 暴露，由 `Message` 触发。
- 新存储：实现 `AppRepository` 并在 `build_service` 处注入。
