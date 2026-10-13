# Windows 用户实际数据下的 PHP 启用状态恢复 — 2026-10-13

## 结果

在用户已安装的 r4 中，通过既有 plan → 界面确认 → apply 重新确认同一个
`R:\App\Env\php\php8.5.11\php.exe`。界面恢复“当前启用 / 当前版本”，重新扫描及
关闭重开后保持；PHP 8.3.35 仍 disabled。未修改应用源码、重装程序、直接编辑
用户数据库、复制另一套状态或创建新安装包。本次为历史状态恢复及验收上下文纠正。

## 现场与根因边界

- 用户截图为 manual / disabled，用户确认曾在 MEnv 启用成功、终端也可用。
- 实际用户库最新成功 enable 为 `1791875935Z`，登记为 `1791879623Z`，扫描时间
  为 `1791879628Z`；lease.enabled=true 且 runtimeId 指向 8.5.11，但 inventory 为 disabled。
  按钮直接呈现当前持久化字段，没有证据证明前端按钮判断错误。
- 当前 Rust scan 与同路径手动登记均保留已有 activation；r4 读取不会自动改写
  已存在的不一致状态。最初是哪次历史写入造成 disabled 尚未追溯证明。
- 安装 EXE 与 r4 独立 EXE 只有 3 字节差异：Tauri bundle type 的 UNK/NSS。
  本地 tauri-utils 2.10.1 的 platform.rs 注明打包会替换该标记，NSS 表示 NSIS；
  不能仅凭整文件哈希不同断言用户运行旧包。

## 必须纠正的验证上下文

工具上下文打开相同逻辑路径时，GetFinalPathNameByHandleW 实际返回：

`C:\Users\17276\AppData\Local\Packages\OpenAI.Codex_2p2nqsd0c76g0\LocalCache\Local\cxszn-menv\state-rust.sqlite3`

这份库显示 detected-unmanaged / active，时间 `1791868793Z`，与截图不一致。
通过已存在的 Windows 桌面 Explorer 上下文启动只读诊断后，句柄实际路径为：

`C:\Users\17276\AppData\Local\cxszn-menv\state-rust.sqlite3`

后者的 manual / disabled 与扫描时间匹配用户截图。普通 Shell.Application
ShellExecute 及工具直接启动的 MEnv 仍继承工具上下文，不能代替桌面用户场景。
本次未更改 Windows/Codex 虚拟化、安全配置或数据根策略。

此前从 Codex 启动的 GUI/CLI 验收属于当时的进程与数据上下文，不能据此推断
用户桌面启动的安装程序使用同一实际库。此前 AppData 文件的“用户状态未变”
对照也受此限制；R 盘 EXE 的文件对照不受该发现影响。

## 操作及验证

沿用原会话对启用同一 8.5.11 路径及扫描复测的明确授权。操作前，以 SQLite
只读连接备份实际用户库；没有通过复制工具库修复状态。

1. 桌面启动已安装 EXE：PID 118532、HWND 9904364，App ID 为实际 EXE 进程。
   初始 UI 明确复现 8.5.11 未启用，同时总览 lease 已生效。
2. 生成 8.5.11 计划并核对同一 R 盘路径，确认后 UI 显示当前启用；数据库记录为
   manual / active，新增成功 enable 时间 `1791880822Z`。
3. 重新扫描完成时间 `1791880853Z`；UI 和实际用户库均保持 active。
4. 关闭并从桌面上下文重开：PID 80540、HWND 9969900。总览与运行时列表均显示
   8.5.11 当前启用/当前版本，8.3.35 仍 disabled。
5. 重启后只读核验：lease.runtimeId 和 PATH 条目摘要与修复前一致。
   安装 EXE SHA-256 保持 `9F7790F0E6DD1663F67D8C0F4439BD87E4AC6E18E933AFF8503EE853B4FB4E9D`。
6. 多余的工具启动实例已关闭，实际用户实例留在运行时列表。

未执行下载安装、升级、外部服务重启或 Git 提交/推送。没有改动生产代码，
因此未重跑无关构建或全量测试；本次证据为真实原生 GUI 与正确用户存储对照。

## 证据

`artifacts/acceptance/windows11/2026-10-13-activation-diagnosis-77e6e321d6374a28958f0947b6a9c913/`

- desktop-state-before.sqlite3：操作前实际用户状态备份，仅本地保留。
- desktop-before.json：实际用户库的 disabled + enabled lease 现场。
- desktop-after-enable.json：确认启用并扫描后。
- desktop-after-restart.json：重启后的物理路径、activation 与 lease 对照。
- read_storage.py：此次只读诊断；已有文件使用 mode=ro，不运行应用 doctor/recovery。
