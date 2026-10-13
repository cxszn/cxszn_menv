export interface PhpIniDiagnostic {
  line: number;
  code: 'NUL_BYTE' | 'INVALID_SECTION' | 'INVALID_DIRECTIVE' | 'DUPLICATE_DIRECTIVE' | 'TRUNCATED_INPUT';
  message: string;
  severity: 'warning' | 'error';
}

export interface PhpIniValidationResult {
  valid: boolean;
  diagnostics: PhpIniDiagnostic[];
}

/**
 * Validate the visible php.ini text without executing PHP or changing the
 * user's file. Unknown directives are intentionally allowed because PHP and
 * extensions add their own keys; this only catches structural mistakes.
 */
export function validatePhpIni(content: string, truncated = false): PhpIniValidationResult {
  const diagnostics: PhpIniDiagnostic[] = [];
  if (truncated) {
    diagnostics.push({
      line: 0,
      code: 'TRUNCATED_INPUT',
      message: '仅显示文件前 512 KiB，无法对完整文件做最终验证。',
      severity: 'warning',
    });
  }
  if (content.includes('\0')) {
    diagnostics.push({
      line: 0,
      code: 'NUL_BYTE',
      message: '文件包含 NUL 字节，不是可安全解析的文本配置。',
      severity: 'error',
    });
  }

  const sections = new Map<string, Set<string>>();
  let section = '';
  content.replace(/^\uFEFF/, '').split(/\r?\n/).forEach((rawLine, index) => {
    const line = index + 1;
    const value = rawLine.trim();
    if (!value || value.startsWith(';') || value.startsWith('#')) return;
    const sectionMatch = /^\[([^\]]+)\]$/.exec(value);
    if (sectionMatch) {
      section = sectionMatch[1]!.trim().toLowerCase();
      if (!section) {
        diagnostics.push({ line, code: 'INVALID_SECTION', message: '配置节名称不能为空。', severity: 'error' });
      }
      return;
    }
    if (value.startsWith('[')) {
      diagnostics.push({ line, code: 'INVALID_SECTION', message: '配置节必须使用 [section] 格式。', severity: 'error' });
      return;
    }
    const directiveMatch = /^([A-Za-z_][A-Za-z0-9_.-]*)\s*=/.exec(value);
    if (!directiveMatch) {
      diagnostics.push({ line, code: 'INVALID_DIRECTIVE', message: '配置行应使用 name = value 格式。', severity: 'error' });
      return;
    }
    const key = directiveMatch[1]!.toLowerCase();
    const keys = sections.get(section) ?? new Set<string>();
    if (keys.has(key)) {
      diagnostics.push({ line, code: 'DUPLICATE_DIRECTIVE', message: `配置节中重复出现 “${directiveMatch[1]}”。`, severity: 'warning' });
    }
    keys.add(key);
    sections.set(section, keys);
  });

  return {
    valid: diagnostics.every((diagnostic) => diagnostic.severity !== 'error'),
    diagnostics,
  };
}
