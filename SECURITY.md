# Security Policy

## Scope

MEnv `0.0.x` is a source/developer preview. It is not a supported Windows
installer and must not be given access to production credentials, machine-wide
configuration, or untrusted runtime packages.

## Reporting a vulnerability

请不要在公开 issue 中发布可复现的本地提权、路径穿越、供应链或密钥泄露
细节。优先通过 GitHub 仓库的私密 security advisory 能力联系维护者；如果该
能力不可用，请先创建不包含敏感细节的 issue，请维护者提供私密沟通方式。

报告至少应包含受影响的 commit/tag、复现条件、影响范围和建议修复方向，且
不得附带真实 Token、私钥、用户目录内容或运行时二进制。

## Supported versions

| Version | Support status |
| --- | --- |
| `0.0.1` | Developer preview; security fixes are best effort |
