# 项目接手盘点（2026-09-22）

## 基线与结论

本次基于本地 `main` 的 `ffd07e8e`，HEAD 标签为 `v10.47.0`，Cargo 版本一致；CHANGELOG 对应日期为 2026-09-15。开始盘点时工作区干净。未查询远端发布状态，也未读取外部任务板；不能据此声称远端部署、发布或工单已关闭。

项目已经是包含 Rust CLI、任务编排、产物托管、可选 HTTP API 和 Swift 客户端的完整系统。当前最需要的是恢复可信的文档和验证基线，并收口生命周期、预算和交付边界，而非重新搭建基础能力。

本次完成目录、配置、发布记录、CI、测试布局的全库盘点，并重点阅读调度、预算、rescue、状态模型、Web 和客户端入口；这是接手基线，不是逐行审计或全套运行验收。后续执行顺序见 [路线图](roadmap.md)。

## 规模与目录职责

统计口径：`git ls-files` 中的文件，行数含注释、空行和内嵌测试；不能当作生产代码量或测试覆盖率。

| 范围 | 数量 | 说明 |
| --- | ---: | --- |
| `src/**/*.rs` | 715 文件 / 136,563 行 | 单一 Cargo package，二进制名 `aid` |
| `tests/**/*.rs` | 23 文件 / 3,843 行 | 包含公共测试辅助模块，不等于 23 个独立测试目标 |
| `client/**/*.swift` | 62 文件 / 5,513 行 | 共享 macOS / iPadOS 源码及测试 |
| `docs/**/*.md` | 53 文件 | 本次修改前的文档基线 |

| 子系统 | 入口 / 核心文件 | 主要职责 |
| --- | --- | --- |
| CLI | `src/main.rs`、`src/cli/`、`src/cmd_dispatch/` | 参数约束、命令分派、启动 |
| 调度 | `src/cmd/run/`、`src/cmd/batch/`、`src/batch/` | 项目上下文、模型选择、DAG、任务启动 |
| Agent | `src/agent/` | 14 个内置 agent、custom registry、命令和事件协议适配 |
| 配额与路由 | `route_availability*`、`live_quota*`、`rate_limit*` | hold、degraded、模型组、配额刷新与候选过滤 |
| 执行与监控 | `background*`、`pty_*`、`watcher/` | detached worker、PTY/流式/缓冲输出、存活与交互 |
| 生命周期 | `task_lifecycle.rs`、`cmd/run/lifecycle.rs`、`types/` | 状态转换、验证、交付判断及后处理 |
| 数据层 | `store/`、`paths.rs` | SQLite、迁移、任务/事件/消息/知识/产物记录 |
| Git 与产物 | `worktree/`、`artifact_custody/`、`commit/`、`cmd/merge*` | 快照、锁、rescue、合并、accept 与 GC |
| 展示 | `cmd/show/`、`board/`、`tui/`、`task_view.rs` | CLI 输出、看板、终端交互 |
| Web | `web/`、`cmd/web.rs` | 可选 `web` feature、鉴权 API、SSE、内嵌静态页 |
| 原生客户端 | `client/AIDCommand/{App,Data,Model,Screens}/` | SwiftUI、实时数据、SSE、Keychain、双平台界面 |
| 运维 | `remote_build/`、`backup/`、`scripts/`、`.github/` | rbox、gws 备份、发布和验证 |
| 操作规范 | `default-skills/aid-guide/`、`.aid/knowledge/` | 对外操作指南与项目内部知识 |

主链路为 CLI → 项目/路由解析 → worker 执行 → watcher/PTY 采集 → 生命周期验证与交付判断 → Store → CLI/TUI/Web/客户端。前台运行也通过 detached worker 执行，再附着观察；不要再按“前台和后台各有一套完整执行器”的旧文档理解。

`TaskStatus` 有 10 个值；`TaskOutcome` 另行表达 verified、delivered、unverified 等判断。`Done` 不等于已经验证，任务终态也不等于可以删除产物。状态转换约束在 `types/status.rs` 和 Store mutation 层，生命周期副作用在 `task_lifecycle.rs` 及 run lifecycle 中。

## 已有能力与旧计划核对

- v10.38.0 已记录 AID Command、鉴权 API/SSE、agy 终态错误识别、跨仓库调度修复；旧路线图的“24 个未发布提交”和 8 月发布门槛已经过时。
- v10.39–10.40 已包含 isolated HOME 链接修复、前台 worker 脱离和旧 job spec 兼容。
- v10.41–10.47 已包含 TUI 查询优化、忽略路径 staging 修复、远程构建与磁盘拒绝重选、备份、状态写入保护、degraded/NeedsHuman 展示及共享 worktree GC 修复。
- `wi-5eef` 已进入 v10.47.0 CHANGELOG，release dry-run hygiene 不应继续排为未实现。
- `wi-e1a0` 的合并错误分层已有 `MergeResult::StashRestoreFailed`、durable stash identity 和对应测试；可从实现队列移到复验队列，但没有外部任务板关闭证据。
- `wi-29bd` 不能概括成“没有预算限制”：`usage.rs` 已限制全局配置中的项目预算；`aid project init/sync` 会把项目预算写入全局配置。剩余问题是同步契约、优先级，以及跨 `--dir` 的项目识别。
- 模块迁移 #152 已完成 run/show/batch 子目录阶段，不能再从这一步重新排期；根目录簇和部分大文件仍需按行为边界整理。

## 需要优先处理的证据

| 优先级 | 发现与证据 | 下一步验收 |
| --- | --- | --- |
| P0 | `cmd/run/dirty.rs` 的 rescue 入口只跳过 read-only/audit，没有 worktree ownership gate；`commit/rescue.rs` 会在传入目录 commit，部分条件下 amend。无 worktree 的调用存在污染主分支风险，尚未动态复现。 | 在隔离测试仓库证明主分支 HEAD、索引和用户改动受保护，产物仍可恢复；覆盖已有提交、无 HEAD、并发/失败路径。 |
| P0 | `dispatch_resolve.rs` 调用 `usage::check_budget_status`；后者通过 `detect_project()` 从 cwd 识别项目，而不是使用本次 dispatch 已解析的目标项目。 | 从 A 目录向 B `--dir` 调度时只应用 B 的项目预算；覆盖目标无配置、linked worktree、retry、batch。 |
| P0 | `.github/workflows/ci.yml` 只做默认 build/test/clippy；`web` 默认关闭。Swift 的两个 scheme 也不在 CI 中。 | 固定同一 SHA 的默认/Web 两种 Rust 验证、live API probe、两平台构建及 Swift 测试证据。 |
| P1 | `.aid/project.toml` 的预算经 init/sync 才进入全局配置；直接修改项目文件的生效规则不清晰。 | 明确配置来源和优先级，覆盖未 sync、修改/移除 cap、窗口边界；区分预算偏好与硬限制。 |
| P1 | 发布 workflow 的构建未启用 `web`；客户端 API probe 需要 `--features web` 的二进制和合适数据库。 | 明确发布产物是否包含 Web、客户端如何配套获取服务端，并提供确定性 fixture，避免依赖个人任务历史。 |
| P1 | 文档存在多个历史路线图；README 链接了被忽略且不存在的 `claude-prompt.md`；内部架构页仍指向已移动文件。 | 本次修正文档入口和明显失效引用；其余历史指南逐条按当前源码重新分诊。 |
| P2 | 结构已拆分但仍有大文件：`rate_limit.rs` 1,214 行、`store/mutations.rs` 963 行、`pty_watch.rs` 912 行、`cmd/run/lifecycle.rs` 673 行。 | 随具体修复抽取职责，不混入状态语义改动；先补相应场景回归。 |
| P2 | `main.rs` 存在 crate 级 dead_code / 多项 clippy allow；仓库跟踪了 `mutants.out/`、`output.txt`、根目录 result 报告。 | 分批收窄 lint 豁免；判定历史证据价值后归档生成物并制定忽略规则。此次不删除。 |

以上风险为源码核对结果，不表示每个分支都已证明故障。尤其预算与 rescue 应先写可复现场景，再修改实现。

## 本次验证及限制

| 检查 | 结果 |
| --- | --- |
| Cargo / HEAD tag / CHANGELOG 对齐 | 均为 v10.47.0 |
| `bash .github/scripts/check-changelog.sh` | 通过 |
| 本次 7 份 Markdown 的本地链接与 `git diff --check` | 通过，无失效文件链接或 diff 空白错误 |
| `python3 scripts/remote-test-test.py` | 未进入测试：本机 Python 3.9.6 不支持脚本使用的 `Path \| None` 注解；需 Python 3.10+ |
| Rust 编译与全套测试 | 未运行：无 rbox、无 `AID_BUILD_BOX`，遵守 CLAUDE.md 远程构建约束 |
| Web live probe | 未运行：尚无本次构建的 Web 二进制和受控 fixture |
| Swift 构建 / 测试 | 未运行：无 XcodeGen，未建立获准的构建环境 |
| ai-board 状态 | 未读取：当前 PATH 无该 CLI；保留原工单 ID，不推测状态 |

历史审计的通过数字只适用于其当时提交，不是当前 HEAD 的验证结果。后续应记录 SHA、工具链、命令、结果和日志位置，建立可重复基线。
