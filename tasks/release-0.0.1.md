# Release 0.0.1 门禁记录

## 发布定位

- 版本：`0.0.1`
- Git 标签：`v0.0.1`
- 发布类型：公开 GitHub source/developer preview（`MEnv 0.0.1 — Source / Developer Preview (Contracts & Skeleton)`）
- 目标仓库：`https://github.com/cxszn/cxszn_menv`
- 目标分支：`main`

此版本只承诺当前已经取得证据的 TypeScript 核心、CLI、stdio MCP、catalog/Skill
契约、Vue 原型和 Rust/Tauri 源码骨架。它不承诺 Windows 11 安装包、真实 PATH
修改、真实下载/安装或 Rust 编译运行。

当前仓库尚未声明开源许可证，发布页必须明确这不是开源授权，也不授权生产使用、
再分发或商用。后续若要作为公开可复用项目发行，需由项目所有者先确定许可证。

## 发布前证据

- [ ] `pnpm test`
- [ ] `pnpm typecheck`
- [ ] `pnpm build`
- [ ] `pnpm lint`
- [ ] `pnpm validate:contracts`
- [ ] catalog/Skill validator
- [ ] MCP schema validator
- [ ] staged diff 无 secrets、Token、私钥和构建产物
- [ ] GitHub remote、分支和 tag 目标已核对

## 发布后核验

- [ ] `git ls-remote` 可看到 `main` 和 `refs/tags/v0.0.1`
- [ ] GitHub release `v0.0.1` 为公开状态，标题和 notes 与 CHANGELOG 一致
- [ ] release 页面明确列出已知限制和下一阶段门禁
- [ ] 本地 tag 指向发布提交

## 回滚

此版本没有数据库迁移和远程部署。若发现源代码/契约问题，优先撤下 GitHub
release，再删除或修正 tag 前先保留当前提交哈希；后续修复使用新的 patch tag，
不改写已发布 tag。
