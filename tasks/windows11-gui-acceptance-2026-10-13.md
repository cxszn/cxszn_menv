# Windows 原生 GUI 实测：0.0.2（2026-10-13）

## 结论

部分通过，整体验收不通过。实际 GUI 可打开，官方 PHP 8.3.35 NTS x64 的确认、下载、安装和未启用展示通过；外部 PHP 8.5.11 的启用日志显示成功，但随后 GUI 仍显示未启用。这不是已修复或正式发行的声明。

本轮按用户“自行测试”授权操作已经打开的应用；安装 PHP 8.3.35 前另外取得了用户明确的行动时许可。本轮仅测试和记录证据，没有修改生产源码、替换现有安装、提交、推送或发布。

## 测试对象与边界

- 实际窗口来自 `R:\App\Code_Tool\cxszn_menv\cxszn-menv.exe`，标题为 MEnv。
- 安装文件版本为 `0.0.2`，大小 13,905,920 bytes，SHA-256 为 `6E47B7D417BE3B9F493616F460E053D9FFD134F46CECC3C0282534C145CDC593`。
- 本机已有用户级 PATH lease；外部 PHP 为 `R:\App\Env\php\php8.5.11\php.exe`。本轮不是无环境干净 VM 验收。
- Windows 界面操作使用 Computer Use；没有通过界面输入或执行终端命令，没有操作系统安全设置或绕过权限提示。
- 命令行检查通过独立非 UI 进程执行，其证据标为 W，不能充当内嵌终端 U 验收。
- 新安装的 PHP 8.3.35 保留在默认受管目录，没有启用、删除或切换它。

## 实测结果

| 项目 | 实际结果 | 判定及证据等级 |
|---|---|---|
| 原生 GUI 启动和导航 | 总览、运行时、活动日志和终端入口可见 | 通过，U；不能以缺少某个 WebView2 注册表入口断言 GUI 不可启动 |
| PHP catalog | 展示 8.2.34、8.3.35、8.4.26、8.5.11、8.1.34、8.0.30、7.4.33；旧版有 EOL/legacy 标识 | 清单展示通过，U；不代表全部版本本轮下载安装通过 |
| 下载计划与取消 | 8.3.35 的版本、x64、NTS 和安装后未启用信息出现在确认页；第一次选择取消，没有发布受管目录 | 通过，U；计划是否在重启后再次出现未测试 |
| 获许可的 GUI 安装 | 用户同意后重新计划并确认 8.3.35；界面显示“已托管、未启用、健康”；活动日志显示安装 succeeded | 通过，W + U，正常路径 |
| 真实安装产物 | `versions\php\8.3.35\x86_64\php.exe` 存在；独立进程实际返回 PHP 8.3.35 NTS x64 | 通过，W |
| 安装 journal | 本轮 journal 记录 planned、staged、downloading、verifying、published、registered | 通过，W；不是失败回滚或崩溃恢复测试 |
| 外部 PHP 跨卷启用 | 确认目标为 R 盘外部 8.5.11 后点击确认；日志新增启用 succeeded，但运行时和总览仍未启用/无活动版本 | 失败，U，阻断用户可见启用验收 |
| 用户级 shim 实际调用 | 在独立检查进程内合并用户/机器 PATH，where、版本和 ini 检查成功；php.target 仍为外部 8.5.11 | 通过，W；没有证明 Explorer 新终端或内嵌终端的行为 |
| 重复 PHP | 两个 8.5.11 TS 条目分别指向外部 PHP 和应用自己的 bin\php.exe；应用自己的条目也能进入启用确认页 | 失败，U；该确认已取消，没有执行自引用启用 |
| 无配置文件查看 | 外部 PHP 的实际 --ini 输出为 `(none)`；界面未显示配置内容，验证和备份按钮不可用 | 空配置显示正常，W + U；有 php.ini 的读取、校验和备份未测 |
| TS/NTS catalog 区分 | 本地是 8.5.11 TS，但 catalog 的 8.5.11 NTS 也被显示为“已登记”，无法从该入口选择安装 | 失败，U，错误合并变体 |
| CLI 与 GUI 一致性 | 同一路径 exe 的 doctor 仍返回 2 个 PHP、16 条操作、8.5.11 active；GUI 已显示 3 个 PHP、3 条活动且无 active | 失败，W + U；分歧原因未确定，不能宣称持久化一致性通过 |
| 可见版本和时间 | 底部仍为 `v0.1 preview`，活动时间为 `1791863313Z` 一类原始秒数 | 展示缺陷，U |
| 内嵌终端 | 只观察页面，未启动会话或自动输入命令 | 未执行 ConPTY 用户会话验收；Computer Use 技能禁止通过 UI 自动执行终端命令 |

## 复现步骤

### 启用成功但仍无活动版本

1. 打开运行时页，筛选 PHP。
2. 选择外部 PHP 8.5.11 的启用按钮。
3. 核对确认页目标是 `R:\App\Env\php\php8.5.11\php.exe`，不是应用的 `bin\php.exe`。
4. 点击确认启用，等待弹窗关闭。
5. 返回总览，观察“暂无活动版本”，同时用户级 shim lease 为“已生效”。
6. 打开活动日志，同一外部目标的最新启用操作为 `succeeded`。
7. 非 UI 的 shim 检查仍可正常运行 8.5.11。

### 重复 PHP 和变体误判

1. 在 PHP 列表观察两个相同版本的 TS 条目。
2. 分别进入启用确认页核对路径；本轮取消了指向 `bin\php.exe` 的计划。
3. 打开下载清单，8.5.11 的包标为 NTS，却因本地 TS 条目被显示为已登记。

## 源码定位（需回归测试，不等于根因修复已完成）

- `crates/application/src/lib.rs:262`：重扫只在既有 ownership 不是 detected-unmanaged 时保留 activation；用户明确启用的外部 detected-unmanaged PHP 也会被扫描结果的 disabled 覆盖。该逻辑与本轮症状吻合，应首先写针对该 ownership 的失败回归测试。
- `apps/tauri/src-tauri/src/lib.rs:333`：只要存在已发布/已登记安装 journal，读取环境状态的恢复路径就会再次 scan_php，然后才判断是否早已登记；这是启用后的读取触发上述逻辑的一条路径。
- `crates/platform-windows/src/php.rs:44`：PATH 候选包含 app-owned shim，未排除自有转发入口；不能用 UI 去重掩盖，也不能让该入口成为启用目标。
- `apps/desktop/src/services/tauriRuntimeService.ts:280`：catalog 的 installed 判断匹配版本和架构，没有匹配 TS/NTS。
- `crates/platform-windows/src/php.rs:428`：ini 解析只接受 `=>`，但本轮实际 --ini 使用 `Loaded Configuration File:`；本轮没有有配置文件样本，不能据此声称该样本已经修复或实测失败。
- `apps/desktop/src/components/AppSidebar.vue:44`：可见版本硬编码为 `v0.1 preview`。

GUI 与 CLI 分歧已经直接观察到；目前尚不能确定是运行进程的状态路径、旧快照、打包差异还是其他原因。修复前要建立能同时检查两个入口的隔离回归，不要直接覆盖用户 SQLite 或 JSON 数据来“同步”。

## 数据保护与安装证据

前后相同的 SHA-256：

| 对象 | SHA-256 |
|---|---|
| 用户 PATH 原文 | `48791EA043A8EF9FF21E582141A37370CF1C7E5ABC9AE9544B177E8813008404` |
| 机器 PATH 原文 | `17ECF12C1EAAABC92465D72319EE9F0EF0CDCD9E804BB812D969EA4E13327C71` |
| 外部 PHP 8.5.11 executable | `AB48ED7BC789D966CBC2AB85C54669FFCD5DD66259AFE94F5B07DB0C75D68A28` |

- 新安装路径：`%LOCALAPPDATA%\cxszn-menv\versions\php\8.3.35\x86_64`。
- 新安装 php.exe SHA-256：`E7BBD16FC7EFD3BD531DA5071C929DC46E54AC2EE14C5483F6DB56759F439751`。
- catalog artifact：`php-8.3.35-nts-x64`，固定来源 `https://downloads.php.net/~windows/releases/php-8.3.35-nts-Win32-vs16-x64.zip`。
- catalog 规定包大小为 33,873,909 bytes，包 SHA-256 为 `25a8e2ac9ff30f1d768d1447c09a600617fa6e6082729f6e95f008b59c91fe45`。这是 ZIP 的预期哈希，不是 php.exe 哈希；本轮没有独立留存 ZIP 再次重算，不能混用两者。
- journal 文件 SHA-256：`742EF54A96FF15477298FD924436DF2B8D7FE722622371919688413929C34CFA`。
- 检查脚本：`scripts/windows/Test-PhpActivationSmoke.ps1 -ExpectedVersion 8.5.11`，实际 `ok=true`；只改变检查进程自身环境，不修改持久 PATH。
- 安装目录、外部 PHP 和 PATH 前后检查完成；没有卸载、清理用户数据、编辑 php.ini 或启动 Web Server。

## 保存的截图

位于 `artifacts/acceptance/windows11/2026-10-13-native-gui/`：

- `php-inventory-installed-disabled.png`：8.3.35 已托管/未启用，两个 8.5.11 TS 条目仍未启用。
- `activity-enable-success.png`：本轮安装和外部 8.5.11 启用成功记录。
- `overview-no-active-with-lease.png`：启用成功后的无活动版本与有效 lease 并存。
- `php-ini-no-config.png`：无配置文件页面。

## 仍未验收

完整内嵌终端交互、旧/新终端快照、切换/停用、下载失败及回滚、跨进程崩溃恢复、无环境干净机、安装器升级/卸载、有效 php.ini 的读取/备份/恢复均未在本轮执行。不得把本轮正常路径安装证据提升为这些场景通过。

下一轮优先处理启用状态与重复 shim、CLI/GUI 状态分歧、TS/NTS 区分，然后针对修复包重复同一 GUI 流程。当前包不具备正式发布验收结论。
