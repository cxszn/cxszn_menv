import { describe, expect, it } from 'vitest';
import { validatePhpIni } from './phpIni';

describe('php.ini validation', () => {
  it('accepts comments, sections and ordinary directives', () => {
    expect(validatePhpIni('; comment\n[PHP]\nmemory_limit = 256M\nextension=\"curl\"')).toMatchObject({
      valid: true,
      diagnostics: [],
    });
  });

  it('reports malformed directives and empty sections as errors', () => {
    const result = validatePhpIni('[PHP]\nnot a directive\n[]');
    expect(result.valid).toBe(false);
    expect(result.diagnostics.map((diagnostic) => diagnostic.code)).toEqual([
      'INVALID_DIRECTIVE',
      'INVALID_SECTION',
    ]);
  });

  it('keeps duplicate keys as warnings and marks truncated input incomplete', () => {
    const result = validatePhpIni('[PHP]\nlog_errors=On\nlog_errors=Off', true);
    expect(result.valid).toBe(true);
    expect(result.diagnostics.map((diagnostic) => diagnostic.code)).toEqual([
      'TRUNCATED_INPUT',
      'DUPLICATE_DIRECTIVE',
    ]);
  });
});
