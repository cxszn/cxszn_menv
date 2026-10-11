import type { SerializedCoreError } from './types.js';

export type CoreErrorCode =
  | 'INVALID_INPUT'
  | 'PATH_NOT_ALLOWED'
  | 'PATH_NOT_FOUND'
  | 'INVALID_RUNTIME_PATH'
  | 'PROBE_FAILED'
  | 'UNSUPPORTED_RUNTIME'
  | 'RUNTIME_NOT_FOUND'
  | 'PLAN_NOT_FOUND'
  | 'PLAN_EXPIRED'
  | 'PLAN_STALE'
  | 'CONFIRMATION_REQUIRED'
  | 'CONFIRMATION_INVALID'
  | 'IDEMPOTENCY_REQUIRED'
  | 'IDEMPOTENCY_CONFLICT'
  | 'PATH_CONFLICT'
  | 'OPERATION_NOT_FOUND'
  | 'OPERATION_NOT_CANCELLABLE'
  | 'PERSISTENCE_ERROR'
  | 'CATALOG_INVALID'
  | 'CATALOG_SIGNATURE_INVALID'
  | 'CATALOG_KEY_UNKNOWN'
  | 'CATALOG_KEY_REVOKED'
  | 'CATALOG_EXPIRED'
  | 'CATALOG_NOT_YET_VALID'
  | 'CATALOG_MIN_APP_VERSION'
  | 'CATALOG_ARTIFACT_REVOKED'
  | 'CATALOG_FIXTURE_NOT_DOWNLOADABLE'
  | 'ARTIFACT_SOURCE_NOT_ALLOWED'
  | 'ARTIFACT_PLATFORM_UNSUPPORTED'
  | 'ARTIFACT_SIZE_MISMATCH'
  | 'ARTIFACT_HASH_MISMATCH'
  | 'STAGING_PATH_INVALID';

export class CoreError extends Error {
  public readonly name = 'CoreError';
  public readonly code: CoreErrorCode;
  public readonly retryable: boolean;
  public readonly nextActions: string[];
  public readonly details: Record<string, unknown> | undefined;

  public constructor(
    code: CoreErrorCode,
    message: string,
    options: { retryable?: boolean; nextActions?: string[]; details?: Record<string, unknown> } = {},
  ) {
    super(message);
    this.code = code;
    this.retryable = options.retryable ?? false;
    this.nextActions = options.nextActions ?? [];
    this.details = options.details;
    Object.setPrototypeOf(this, new.target.prototype);
  }

  public toJSON(): SerializedCoreError {
    return {
      code: this.code,
      message: this.message,
      retryable: this.retryable,
      nextActions: [...this.nextActions],
      ...(this.details === undefined ? {} : { details: this.details }),
    };
  }
}

export function asCoreError(error: unknown, fallbackCode: CoreErrorCode = 'PERSISTENCE_ERROR'): CoreError {
  if (error instanceof CoreError) return error;
  const message = error instanceof Error ? error.message : String(error);
  return new CoreError(fallbackCode, message, { retryable: true });
}
