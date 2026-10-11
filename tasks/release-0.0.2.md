# Release 0.0.2 门禁记录（未发布）

## 当前定位

`0.0.2` 当前是 Windows 原生能力候选，不是已发布版本。正式 release 仍被
signed catalog/install、ConPTY child I/O、Job Object、PHP provider/Tauri
生命周期接线、许可证和 installer signing 阻断。

## 已取得证据

- [x] Windows 11 x64 host preflight：Build 26300。
- [x] `cargo check --workspace`。
- [x] `cargo test --workspace`。
- [x] `cargo clippy --workspace --all-targets -- -D warnings`。
- [x] `pnpm test/typecheck/build/lint/validate:contracts`。
- [x] catalog Ed25519/staging/size/SHA-256 negative tests。
- [x] HKCU PATH + hard-link shim roundtrip，恢复用户 PATH，机器 PATH 未变。
- [x] unsigned NSIS current-user installer build。
- [x] unsigned installer install/launch/uninstall smoke。

## 阻断项

- [ ] ConPTY 真实 child I/O 输出和进程树回收在 Win11 主机通过。
- [ ] Job Object 集成并取得 timeout/cancel/child-tree 证据。
- [ ] 真实 signed catalog trust root、HTTPS download、Zip-safe extraction、
      immutable PHP registration 和 rollback。
- [ ] Windows PHP provider scan/manual path 与 Tauri runtime state 接线。
- [ ] installer Authenticode signing、证书链和签名验收。
- [ ] 普通用户完整 GUI/U 级验收和升级/回滚验收。

在全部阻断项关闭前，不创建 `v0.0.2` tag/release。
