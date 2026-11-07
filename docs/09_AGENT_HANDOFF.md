# Astra × Opus 5.5 并行开发协定

**目标：** 两位 Agent 尽可能并行产出，而不是频繁互相等待或重复实现。架构决策/能力接口由 Astra 负责提案，前端交互与视觉由 Opus 5.5 主导；接口契约必须由双方共同确认。

## 分工与代码所有权

| 模块 | 主负责人 | 协作者 | 保护边界 |
|---|---|---|---|
| `crates/nekopak-domain` | Astra | Opus 5.5 评审 TS 类型 | 合约变更必须同步双方 |
| `crates/nekopak-core`, `provider-*`, `sync-*` | Astra | Opus 5.5 提交需求 | Opus 不直接重写网络、鉴权、同步核心 |
| `src-tauri` | Astra | Opus 5.5 集成 | 无未经评审的 Tauri API 变化 |
| `src`, `public`, `ui` | Opus 5.5 | Astra mock 支持 | Astra 不直接重做整个 UI |
| `docs/03_CONTRACTS.md` | 共同维护 | 共同维护 | 先变更契约再变更实现 |
| 许可证来源、NOTICE | Astra | Opus 提供 UI 素材来源 | 任何复制行为须注明 |
| CI/打包体积与稳定性 | Astra | Opus 压缩静态资源 | 对所有产物提供可复现结果 |

## 第一次并行开发（允许不依赖真实服务）

1. Astra 创建工作空间和 `MockProvider`，提供 `files_list`, `backend_discover`, `transfers_list`, `transfer_control`, `cloud_task_create` 的 mock 实现，按 `03_CONTRACTS.md` 输出类型与测试。
2. Opus 5.5 创建 Tauri React/TypeScript 可选的前端设计，只对接 `src/lib/api.ts` 抽象，不擅自变更后端业务模型；使用至少七组 mock 错误场景。
3. 双方提交状态：完成文件、测试命令、阻塞项、下一 PR；任何“不支持”操作必须在 UI 上禁用。
4. 合并前跑契约测试和 E2E；用户确认产品流程后再启用真实 PikPak 会话。

## Git 分支建议

```text
main                      # 集成、可运行状态
feat/core-foundation      # Astra
feat/mock-provider        # Astra
feat/ui-shell             # Opus 5.5
feat/files-explorer       # Opus 5.5
feat/cli-adapter          # Astra (after audit)
feat/sync-plan            # Astra (later)
```

## 交接模板（每个阶段必填）

```markdown
### 完成
- 文件与 commit:
- 运行/构建命令:
- 测试结果（确实执行过）:
- 体积/内存变化：
### 待处理
- 仍未实现（不要伪称已完成）:
- 风险/许可证核查:
- 接口变化（有/无）:
### 供对方下一步使用
- 可调用命令：
- Mock fixtures：
- 下一任务：
```

## 反复发生时立即阻断的错误

- 未经沟通改动命令/字段名导致前后端不兼容。
- 复制 AGPL 项目代码后把主仓发布为 Apache；未注明来源。
- 将“官方 CLI 能下载”推断为“已支持随机读取和真实挂载”。
- 为缩小体积删除 TLS 验证、可靠状态存储、任务校验。
- 把反向工程调用描述成 PikPak 官方公开 REST API。
- 展示伪造账号、配额和不存在的云下载进度。
- 未经确认自行改 Hosts、安装驱动、提升权限或规避平台规则。
