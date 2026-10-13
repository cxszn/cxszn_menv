# MEnv 0.0.2 Windows x64 开发者预览版

本版本为未签名预览版（NotSigned），不是完成全部验收的正式版。

## 下载与运行

- `cxszn_menv_0.0.2_x64-setup.exe`：当前用户安装器。内嵌 WebView2 引导程序；
  如果电脑缺少 WebView2 Runtime，安装时仍需要联网获取它。
- `cxszn_menv_0.0.2_windows-x64.zip`：解压后运行 `cxszn-menv.exe`。
  电脑需已安装 Microsoft Edge WebView2 Runtime。这个 ZIP 不包含离线 Runtime。
- ZIP 只是免安装程序包；状态和受管运行时仍使用当前用户的
  `%LOCALAPPDATA%\cxszn-menv`，不会自动变成随 U 盘移动的数据模式。

下载后可将文件 SHA-256 与版本页的 SHA256SUMS.txt 对照。
如 Windows 显示安全提示，请自行判断；本说明不要求关闭安全保护。

## 本版能力

PHP 只读扫描、手动路径登记、启用/停用/切换、签名清单下载和受管安装、配置读取与
确认备份已接入原生面板。当前清单包含 PHP 7.4 及 8.0–8.5 的部分 x64 构建；
旧版会显示 legacy/EOL 提示。Node、Python、MySQL client 目前仅检测。

所有写操作采用计划、确认和执行流程。原生命令行的 `change confirm --json`
要求真实控制台输入和 stderr；检查显示内容后输入完整 `approve <计划 UUID>`。
管道、EOF、未签发 token 和旧未绑定登记计划不能绕过批准。

待安装计划可持久取消；已经开始下载/安装的计划不能用此入口中断。错误时
“关闭（保留计划）”只收起弹窗，不表示操作已取消。切换 PHP 只影响新终端。

## 升级与已知限制

- 升级前保留现有用户数据。若旧版留下“lease 已生效、运行时未启用”的不一致，
  核对具体 PHP 路径后，通过本版正常确认流程重新启用；不要手工合并不同状态库。
- 从打包宿主或工具启动时，Windows 可能重定向 AppData。排障必须核对实际数据
  文件路径，不能假定工具启动与从 Windows 桌面启动共享同一份状态。
- 完整下载/安装崩溃恢复、内嵌终端、升级/卸载及干净机验收仍未全部完成。
- 此包不附带 PHP/Node/Python/MySQL 二进制，不自动接管机器级 PATH 或外部服务。

项目尚未选择开源许可证，第三方组件的许可与声明见 THIRD-PARTY-NOTICES.md。
源代码、详细验收范围和问题反馈入口：
https://github.com/cxszn/cxszn_menv
