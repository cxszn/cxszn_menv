# MEnv 0.0.2 Windows 原生实施计划

## 目标

将 `0.0.1` 的源码/契约预览推进到可在 Windows 11 x64 实机验收的首个原生
能力切片，优先覆盖：

1. HKCU PATH lease 与 app-owned shim。
2. ConPTY 新会话和受控进程生命周期。
3. 签名 catalog、HTTPS 下载、staging、SHA-256、Zip 安全和原子注册。
4. Tauri current-user NSIS 安装包。
5. Windows 11 x64 clean-machine 验收和 `v0.0.2` 发布。

## 架构迁移门槛

- Rust `crates/application` 是 Tauri 原生能力的唯一 application service；
  Tauri command 只能调用该 service。
- TypeScript `packages/core` 在 `0.0.2` 保留 CLI/MCP 兼容实现，但任何新增
  Windows 写操作必须先在 Rust service 建立等价契约；不得出现第三套 PATH、
  安装或回滚逻辑。
- Windows adapter 只依赖 domain/ports，不把注册表、ConPTY 或 Tauri 类型
  泄漏到 domain。
- 写操作仍遵循 `plan -> host-issued token -> apply`，并持久化 plan hash、
  token hash、状态快照、操作锁、journal 和回滚点。

## 切片顺序

### Slice A — PATH lease/shim

- 读取 HKCU `Environment\Path`，保留未知条目和原始顺序。
- 只写应用自己的 lease；拒绝 HKLM、PowerShell profile、第三方条目和 UNC。
- 受管 PHP 版本使用固定 `%LOCALAPPDATA%\\cxszn-menv\\bin` shim，shim 目标
  只能是已经校验的绝对 `php.exe`。
- 写入后读回、哈希校验并发送 `WM_SETTINGCHANGE`；并发哈希冲突时拒绝覆盖。
- 验收停用只移除自身 lease，已有终端不热切换，新终端使用新快照。

### Slice B — ConPTY

- 通过 `CreatePseudoConsole`、同步管道和 `STARTUPINFOEX` 创建受控会话。
- executable、argv、cwd、环境变量均为结构化参数；禁止 shell 字符串。
- 独立读写线程、输出上限、启动/空闲/总超时和 Job Object 进程树限制。
- 切换只影响新会话，关闭会话不杀其他运行时进程。

### Slice C — signed catalog/install

- 内置运行时信任根与应用更新信任根分离。
- Ed25519 签名、key id、有效期、撤销、最小安全版本、固定域名/redirect、
  SHA-256、大小、架构和入口文件全部 fail-closed。
- 下载只进入隔离 staging；拒绝 Zip Slip、绝对路径、UNC、reparse/symlink
  escape、压缩炸弹和覆盖已存在版本。
- 健康探测完成后原子 rename 到不可变版本目录，失败只清理 staging 并恢复
  last-known-good。

### Slice D — Tauri installer

- current-user NSIS，默认不要求管理员权限；bundle 只打包已取得证据的应用。
- CI 使用 Windows runner 执行 `pnpm install --frozen-lockfile`、前端构建、
  `cargo check` 和 `tauri build`。
- 没有签名证书时安装包必须标记 unsigned，不得伪造签名状态。

### Slice E — Win11 x64 acceptance/release

- 在干净 Windows 11 x64 机器记录 OS build、架构、Rust/MSVC/WebView2、安装包
  SHA-256 和操作日志。
- 逐项执行扫描、手动路径、PATH 冲突、shim、终端快照、ConPTY、catalog 失败、
  安装/卸载、回滚和新终端验证。
- 只有静态、CI、实机和用户可见证据全部分层记录后，才创建 `v0.0.2`。

## 当前明确不声称

- 本计划完成前，不声称 `0.0.2` 已可用、不上传 PHP 二进制、不修改用户机器
  PATH、不执行真实安装或重启外部服务。
- 本机没有 Rust/MSVC 工具链时，Rust 编译和 Win11 验收状态必须保持未验证。`r`n