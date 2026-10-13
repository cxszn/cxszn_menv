# ADR-0014: Preserve explicit activation and share native snapshot recovery

## 状态

已接受；适用于 0.0.2 修复预览。自动化证据已取得，新包用户可见验收仍需单独记录。

## 背景

2026-10-13 原生 GUI 实测发现：启用外部 PHP 后日志成功，但刷新仍显示未启用；
PATH 中应用自己的 shim 被识别成独立 PHP；本地 TS 构建被错误视为已安装的 NTS 包。
GUI 与原生 CLI 观察到不同快照，旧包的分歧来源尚未完全证明。

## 决策

- ownership 只描述安装来源。扫描同一 executable 时保留用户明确选择的 activation
  和 first-seen 信息，包括 detected-unmanaged，不通过自动登记改变外部文件归属。
- 已启用/过渡中的外部记录即使暂时不在扫描结果中也保留；扫描不是隐式停用。
- application service 从扫描结果和历史 inventory 中排除自己的 PHP shim，并在
  enable/switch 的 plan 与 apply 阶段拒绝自身 shim；Windows provider 在探测前拒绝
  此入口。`bin-extra` 等相似外部路径不能按前缀误排除。
- ShimManager 在文件变更前拒绝自引用；launcher 在创建子进程前再次拒绝自引用。
  比较规范化路径和可解析的 canonical 路径，不比较 file identity：同盘 shim 本来
  就与合法目标共享 hard-link identity，按 identity 拒绝会破坏正常启用。
- 外部 hard-link/copy 别名也可能包装 launcher。首次启用时按受限流式 SHA-256
  比较目标与当前宿主；已有 shim 时比较目标、owned shim 与记录中的实际 PHP，
  拒绝 launcher 别名，同时保留正常 PHP hard-link 再启用。比较限额 128 MiB，
  中途大小变化或 I/O 错误 fail-closed，且拒绝发生在 PATH/文件发布前。
- 完成登记的安全 journal 只做一致性读取；只有存在尚未登记的安全 entrypoint 才
  执行恢复扫描。普通状态读取不会反复改变已完成快照或使确认计划失效。
- GUI 与原生 CLI 的 doctor/runtimes list 共用 `load_native_environment_state`，包括
  named mutex、journal 恢复与同一个 application service 的状态读取。没有新增
  公共 RPC、schema、下载来源、提权能力或生产依赖。
- catalog 的安装状态按版本、架构和 TS/NTS 构建区分；unknown thread safety 不能
  证明某个已签名构建已经安装。ini 探测兼容实际 CLI 的冒号标签和已有箭头格式。

## 取舍

- 不在 UI 中根据 lease 强行补出 active 状态；那会掩盖底层快照错误。
- 不按版本号去重 PHP；不同路径、架构和 TS/NTS 是有效的独立安装。
- 不覆盖用户数据库或删除旧 JSON 来处理入口分歧；保留源数据，用同一新 EXE 的
  GUI/CLI 对照、哈希和操作记录继续核实。
- journal 仍是独立 JSONL 恢复源；本次不宣称已实现完整跨文件事务或崩溃回滚。

## 验证

- 2026-10-13 补充：比较 GUI/CLI 时先核对文件句柄实际路径；Codex AppData 重定向
  会产生相同逻辑路径但不同物理库的情况。本次用户现场恢复及证据边界见
  `windows11-activation-user-context-2026-10-13.md`。历史库不一致不能通过 UI 强行补 active。
- enable → scan → get 的真实编排回归、旧 shim inventory 排除、enable/switch
  自目标拒绝均先取得红灯，再通过；扫描未写 PATH。
- 自引用 shim fixture 先证明旧实现会改写目标记录，再验证拒绝且 owned pair 不变。
- Tauri 隔离 SQLite/安全 journal fixture 先证明 getter 会改变快照，再验证完整快照不变。
- TS/NTS/unknown 变体回归先失败；反转匹配条件的 mutation 被三项测试捕获，恢复后通过。
- `php --ini` 冒号标签回归先失败，修复后保留盘符冒号并正确识别 `(none)`。
- 初始 full checks：Rust 82 passed / 8 原有 ignored；TypeScript 75 passed，类型检查、
  构建、clippy、MCP 契约通过。没有新增 ignored 或跳过失败断言。
- 新包 GUI、真实 enable/scan/restart、CLI 对照与原生安装器验收必须继续记录，不用
  此 ADR 或自动化检查替代 W/U 证据。
- 独立复审先阻断外部 launcher alias 与首次启用绕过；追加两个红灯回归并修复后，
  shim 的九项聚焦测试通过，包括真实 NTFS hard link、copy 和合法同盘再启用。
  复审批准代码切片，用户可见新包与 CLI 对照仍为 WATCH。
