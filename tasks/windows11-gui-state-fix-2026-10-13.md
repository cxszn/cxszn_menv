# 0.0.2 GUI 状态修复切片（2026-10-13）

## 结论

已实现并通过自动化/原生 CLI 检查，生成 r2 unsigned 测试包；用户可见复测未完成，完整目标保持 active。没有执行安装器、覆盖原安装、提交、推送或发布。

前一轮 GUI 实测是进展：它复现了启用成功但仍 disabled、自有 shim 重复、TS/NTS 合并和 GUI/CLI 快照分歧，改变了本轮修复的优先级。本轮进一步完成源码、红/绿回归、安全复审和新交付物，不能归类为状态重述。

## 修复及证据

- Application enable → scan → get：明确启用的 detected-unmanaged 仍 active，ownership 和 firstSeenAt 保留，未隐式登记为 manual；扫描不写 PATH。
- 扫描过滤自身 shim，也移除历史 inventory 中的该伪运行时；相似的 `bin-extra` 外部路径保留。旧 inventory 上的 enable/switch 计划与 apply 均拒绝自身 shim。
- Windows provider 在执行健康探测前拒绝 app-owned shim，实际 r2 CLI 调用验证该路径返回 `PLATFORM_ERROR`。
- 初始路径级 shim 防护通过后，独立复审仍发现外部 hard-link/copy alias 风险。先新增两个失败 fixture，再补内容身份防护：首次 activation 拒绝宿主副本/硬链接；已有 launcher 的外部别名不能替换 owned pair；合法同盘 PHP hard link 可再次启用；launcher 在 spawn 前再次校验。
- 受限 SHA-256 比较使用既有依赖、8 KiB 缓冲和 128 MiB 上限；大小变化与 I/O 错误 fail-closed。
- 已登记的安全 install journal 不触发重扫；隔离 SQLite/真实安全路径 fixture 先证实旧 getter 会改变快照，再验证读后完整快照不变。
- GUI 与 native CLI doctor/runtimes list 使用同一锁和恢复入口。旧包 GUI/CLI 分歧的全部来源仍未证明；新包还须做实际跨入口 U 对照，不能仅凭共用函数宣称解决所有分歧。
- catalog 已安装判断补 TS/NTS 匹配；TS/unknown 不阻止下载 NTS。反转匹配条件时三项变体测试均失败，恢复后通过。
- `php --ini` 实际冒号标签先红后绿；盘符冒号保留，真实外部 8.5.11 的 ini 来源现在正确为 none。
- Sidebar 从 package version 获取版本号，构建产物不再硬编码 v0.1。

## 验证命令与结果

| 验证 | 结果与范围 |
|---|---|
| cargo test --workspace | 84 passed / 8 原有 ignored；本轮未新增 ignored/删除断言 |
| cargo clippy --workspace --all-targets -- -D warnings | 通过；新增测试 initializer 告警已修正，未抑制规则 |
| cargo fmt --all --check | 通过 |
| pnpm test | 75 passed |
| pnpm typecheck/build/lint/validate:contracts | 通过；8 个 MCP tool 契约有效 |
| 最新 Sidebar 改动后的 desktop test/typecheck 与 Tauri beforeBuild | 再次通过，21 desktop tests |
| pnpm audit --prod --registry=https://registry.npmjs.org --json | 25 个 production/optional 依赖，无已知 advisory；默认镜像 audit endpoint 不存在，临时官方 registry 参数未改本地配置 |
| pnpm --dir apps/tauri bundle:windows | r2 release/NSIS 构建成功 |
| r2 EXE doctor / runtimes list --json | ok=true，两次序列化 result 完全一致；有 8.3.35 managed/disabled 和外部 8.5.11，无自有 shim 记录 |
| r2 EXE runtime verify 外部 PHP --json | ok=true，8.5.11，ini source=none；仅读，不执行 enable |
| r2 EXE runtime verify 自有 shim --json | 拒绝，PLATFORM_ERROR；没有把转发程序当作独立 PHP |
| r2 EXE catalog list --kind php --json | 7 个 artifact |
| Test-PhpActivationSmoke.ps1 -ExpectedVersion 8.5.11 | ok=true；既有转发调用仍正常，不等于新 EXE 的 GUI 通过 |

安全复审：`.omo/evidence/cxszn-menv-gui-state-code-review.md`，代码切片 APPROVE、codeQualityStatus WATCH。WATCH 明确保留真实新包 GUI 与 CLI 对照门禁。

## 新包

目录：`artifacts/cxszn_menv-windows-x64-nsis-unsigned-0.0.2-gui-state-fix-20261013-r2/`。

- NSIS：5,272,275 bytes；SHA-256 `3567E9305BF5C165073BAA6051D54AC0CF3428A399D68A2C97D455068E92D951`；NotSigned。
- EXE：13,958,144 bytes；SHA-256 `C131E343A55EAD4DC02BCE7C3671D3D06F6093083234A06DD662D3D8702A7AF2`；NotSigned。
- 不带 `-r2` 的第一候选已在 manifest 标记 superseded，不能充当最终安全修复候选。

## GUI 复测限制及下一步

用户已批准启动独立修复版，不运行安装器、不覆盖现有安装。电脑工具第一次绝对路径启动后出现了同安装路径的额外进程；显式 process 路径启动后，W 盘第一候选的进程确实存在，但没有可按该进程路径唯一选定的 native Window。

12:21 的工具 inventory 中：原安装的 MEnv 窗口仍归属于 `process:R:\App\Code_Tool\cxszn_menv\cxszn-menv.exe`；另有两个标题为 MEnv 的窗口归属于 OpenAI.Codex。Computer Use 技能禁止操控 Codex UI，不能仅凭同名标题把后者当作修复版应用。未捕获/操控这些 Codex 窗口，未用其画面做 U 证明。第一候选进程没有被当作已成功显示 GUI，未重复盲目启动。

请用户在 Windows 中直接打开 r2 独立 EXE（不是 Codex 预览），待工具返回该 EXE 的真实窗口后再做：8.5.11 确认启用 → 重新扫描 → 重启 → 新包 CLI 对照；验证只有两个真实 PHP，NTS 8.5.11 可选，8.3.35 仍 disabled。内嵌终端命令不通过 Computer Use 自动输入。

当前真实默认 SQLite 的外部 8.5.11 activation 仍为 disabled，但 lease 与 php.target 仍有效且实际命令返回 8.5.11。没有根据 lease 静默伪造 active，也没有覆盖数据库来同步状态；需下一轮明确确认启用完成状态修复与验证。

## 用户数据保护

12:19 核对的哈希仍与此前相同：

- 用户 PATH：`48791EA043A8EF9FF21E582141A37370CF1C7E5ABC9AE9544B177E8813008404`。
- 机器 PATH：`17ECF12C1EAAABC92465D72319EE9F0EF0CDCD9E804BB812D969EA4E13327C71`。
- 外部 PHP：`AB48ED7BC789D966CBC2AB85C54669FFCD5DD66259AFE94F5B07DB0C75D68A28`。
- 原安装 EXE：`6E47B7D417BE3B9F493616F460E053D9FFD134F46CECC3C0282534C145CDC593`。

没有切换到新下载的 8.3.35，没有修改 php.ini、机器 PATH、Profile、外部服务或现有安装。

## 全目标门禁

新包 GUI、原生安装器/升级/卸载、干净机、下载失败/安装回滚、跨进程崩溃恢复、完整 ConPTY 用户会话与许可证等门禁仍需继续。当前结果是具体修复进展，不是“请继续完善版本”目标完成或正式发布。

## 后续复测更新

用户说明先前已关闭窗口，并再次授权打开 r2、确认启用现有 8.5.11 和启动 r3。
r2 的真实 GUI 启用、重新扫描保持 active 与 CLI/shim 对照已经通过；r3 重启读取、
TS/NTS catalog 下载入口与隔离 native CLI ini 备份也已通过。完整系统门禁未完成。
后续以 `tasks/windows11-r3-acceptance-2026-10-13.md` 的逐级证据为准，保留上面的
历史限制与旧候选哈希，不将它们当作当前仍缺少窗口的结论。
