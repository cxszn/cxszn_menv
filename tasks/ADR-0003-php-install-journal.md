# ADR-0003: PHP catalog install journal and recovery boundary

## 状态

已接受，适用于 0.0.2 Windows PHP catalog 安装切片。

## 背景

PHP ZIP 下载、校验、解压和原子发布跨越网络、磁盘和进程生命周期。进程可能在
managed 目录发布后、环境快照登记前退出。仅依赖目录扫描会让用户无法区分“已发布
但未登记”和“未安装”，也无法提供恢复依据。

## 决策

- 每个 catalog PHP 安装可以绑定一个应用自有 `.jsonl` journal。
- journal 只记录 schema 版本、阶段、时间、文件名、期望 SHA-256、managed 目录和
  脱敏错误文本；不记录 Token、完整环境变量或原始命令输出。
- adapter 阶段顺序为：`planned`、`staged`、`downloading`、`verifying`、`published`、
  `failed`。
- Tauri 完成环境扫描和 managed/disabled 登记后追加 `registered`。
- journal 使用追加写入和 `sync_all`；安装 staging 使用唯一操作目录，成功和失败均
  清理 staging。
- 如果 managed 目录已发布但 `published` journal 写入失败，adapter 会删除本次新目录
  后返回失败；只有在回滚删除失败时才可能留下需要人工诊断的 orphan。
- 应用读取状态时，只扫描 `%LOCALAPPDATA%\cxszn-menv\operations\php` 根目录及其
  一层版本子目录中的 journal，且只恢复位于 `%LOCALAPPDATA%\cxszn-menv\versions\php`
  内的绝对路径；这兼容旧版根级 journal 和当前 `operations\php\<version>\install.jsonl`
  布局。
- 损坏 journal、UNC、父级穿越和 managed 根外路径直接跳过；不会接管用户手动路径。
- 恢复前逐级检查 managed 根到 `php.exe` 的现有组件，拒绝 symlink/reparse point
  和非普通文件，避免 journal 被篡改后跟随 junction 越界。

## 未解决边界

- journal 不是跨进程事务锁；并发由当前 Tauri install I/O lock 限制，跨进程锁和
  SQLite 事务仍属于后续持久化切片。
- journal 恢复已具备 adapter/Tauri 接线，但崩溃注入、GUI 回滚和干净机 W/U 证据仍未完成。

## 验证

- 七个 PHP x64 catalog artifact 完成真实下载、大小/SHA-256、ZIP 安全解压、原子发布、
  `php -n -v` 和 `published` journal 验收。
- Rust workspace、clippy、TypeScript/Vue 测试和 Tauri bundle 均通过；新增
  journal recovery 根边界、嵌套版本目录和普通文件测试通过。
