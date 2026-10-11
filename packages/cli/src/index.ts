import { randomUUID } from 'node:crypto';

import { runMcpServe } from './mcp-entry.js';

/**
 * The CLI deliberately depends on this small port instead of importing any
 * platform or persistence implementation. The core package can satisfy this
 * port directly, while tests and future hosts can provide an adapter.
 */
export interface CliCorePort {
  getEnvironmentState(): Promise<unknown> | unknown;
  scanInstalledRuntimes(input?: unknown): Promise<unknown> | unknown;
  listRuntimeCatalog(kind?: string): Promise<unknown> | unknown;
  verifyRuntimePath(path: string, family?: string): Promise<unknown> | unknown;
  planEnvironmentChange(input: unknown): Promise<unknown> | unknown;
  confirmPlan(planId: string, actor?: string): Promise<unknown> | unknown;
  applyEnvironmentChange(input: {
    planId: string;
    planHash?: string;
    confirmationToken?: string;
    idempotencyKey: string;
    actor?: string;
  }): Promise<unknown> | unknown;
  getOperationStatus(operationId: string): Promise<unknown> | unknown;
  cancelOperation(operationId: string): Promise<unknown> | unknown;
}

export interface CliDependencies {
  core: CliCorePort;
  requestId?: () => string;
  write?: (line: string) => void;
}

type SuccessEnvelope = {
  schema_version: '1';
  request_id: string;
  ok: true;
  result: unknown;
  operation_id?: string;
};

type ErrorEnvelope = {
  schema_version: '1';
  request_id: string;
  ok: false;
  error: {
    code: string;
    message: string;
    retryable: boolean;
    next_actions: string[];
  };
};

export type CliEnvelope = SuccessEnvelope | ErrorEnvelope;

type ParsedOptions = {
  positional: string[];
  values: Map<string, string>;
  flags: Set<string>;
};

class CliInputError extends Error {
  readonly code: string;
  readonly retryable: boolean;
  readonly nextActions: string[];

  constructor(code: string, message: string, nextActions: string[] = []) {
    super(message);
    this.name = 'CliInputError';
    this.code = code;
    this.retryable = false;
    this.nextActions = nextActions;
  }
}

function parseOptions(tokens: string[]): ParsedOptions {
  const positional: string[] = [];
  const values = new Map<string, string>();
  const flags = new Set<string>();

  for (let index = 0; index < tokens.length; index += 1) {
    const token = tokens[index];
    if (token === undefined) {
      continue;
    }
    if (!token.startsWith('--')) {
      positional.push(token);
      continue;
    }
    const equalsIndex = token.indexOf('=');
    const rawName = equalsIndex === -1 ? token.slice(2) : token.slice(2, equalsIndex);
    if (!rawName) {
      throw new CliInputError('INVALID_ARGUMENT', 'Option name cannot be empty.');
    }
    if (equalsIndex !== -1) {
      const value = token.slice(equalsIndex + 1);
      if (!value) {
        throw new CliInputError('INVALID_ARGUMENT', `Option --${rawName} requires a value.`);
      }
      values.set(rawName, value);
      continue;
    }
    const next = tokens[index + 1];
    if (next !== undefined && !next.startsWith('--')) {
      values.set(rawName, next);
      index += 1;
      continue;
    }
    flags.add(rawName);
  }

  return { positional, values, flags };
}

function option(options: ParsedOptions, name: string): string | undefined {
  return options.values.get(name);
}

function aliasedOption(options: ParsedOptions, primary: string, alias: string): string | undefined {
  const primaryValue = option(options, primary);
  const aliasValue = option(options, alias);
  if (primaryValue !== undefined && aliasValue !== undefined && primaryValue !== aliasValue) {
    throw new CliInputError('INVALID_ARGUMENT', `Options --${primary} and --${alias} must match when both are provided.`);
  }
  return primaryValue ?? aliasValue;
}

function requiredOption(options: ParsedOptions, name: string): string {
  const value = option(options, name);
  if (!value) {
    throw new CliInputError('INVALID_ARGUMENT', `Missing required option --${name}.`);
  }
  return value;
}

function rejectUnexpectedOptions(options: ParsedOptions, allowed: ReadonlySet<string>): void {
  for (const name of options.values.keys()) {
    if (!allowed.has(name)) {
      throw new CliInputError('INVALID_ARGUMENT', `Unknown option --${name}.`);
    }
  }
  for (const name of options.flags) {
    if (!allowed.has(name)) {
      throw new CliInputError('INVALID_ARGUMENT', `Unknown option --${name}.`);
    }
  }
}

function extractOperationId(result: unknown): string | undefined {
  if (typeof result !== 'object' || result === null) {
    return undefined;
  }
  const record = result as { operationId?: unknown; operation_id?: unknown };
  const operationId = record.operationId ?? record.operation_id;
  return typeof operationId === 'string' && operationId.length > 0 ? operationId : undefined;
}

function successEnvelope(requestId: string, result: unknown): SuccessEnvelope {
  const operationId = extractOperationId(result);
  return operationId === undefined
    ? { schema_version: '1', request_id: requestId, ok: true, result }
    : { schema_version: '1', request_id: requestId, ok: true, result, operation_id: operationId };
}

function errorEnvelope(requestId: string, error: unknown): ErrorEnvelope {
  if (error instanceof CliInputError) {
    return {
      schema_version: '1',
      request_id: requestId,
      ok: false,
      error: {
        code: error.code,
        message: error.message,
        retryable: error.retryable,
        next_actions: error.nextActions,
      },
    };
  }

  const candidate = typeof error === 'object' && error !== null ? (error as Record<string, unknown>) : {};
  const code = typeof candidate.code === 'string' ? candidate.code : 'CORE_ERROR';
  const message = typeof candidate.message === 'string' ? candidate.message : 'Core operation failed.';
  const retryable = typeof candidate.retryable === 'boolean' ? candidate.retryable : false;
  const rawNextActions = candidate.next_actions ?? candidate.nextActions;
  const nextActions = Array.isArray(rawNextActions)
    ? rawNextActions.filter((item): item is string => typeof item === 'string')
    : [];
  return {
    schema_version: '1',
    request_id: requestId,
    ok: false,
    error: { code, message, retryable, next_actions: nextActions },
  };
}

function changeInput(options: ParsedOptions, action?: string): Record<string, unknown> {
  const selectedAction = action ?? requiredOption(options, 'action');
  if (!selectedAction) {
    throw new CliInputError('INVALID_ARGUMENT', 'A change action is required.');
  }
  const input: Record<string, unknown> = { action: selectedAction };
  const fields = [
    ['runtime-id', 'runtimeId'],
    ['target-runtime-id', 'targetRuntimeId'],
    ['path', 'path'],
    ['scope', 'scope'],
    ['actor', 'actor'],
  ] as const;
  for (const [optionName, key] of fields) {
    const value = option(options, optionName);
    if (value !== undefined) {
      input[key] = value;
    }
  }
  if (input.scope === 'process') {
    throw new CliInputError('UNSUPPORTED_SCOPE', 'Process scope is host-only until ConPTY/session environment injection is verified.');
  }
  return input;
}

async function executeCommand(core: CliCorePort, command: string[], options: ParsedOptions): Promise<unknown> {
  const [first, second] = command;
  if (first === undefined) {
    throw new CliInputError('INVALID_ARGUMENT', 'A command is required.');
  }

  if (first === 'doctor') {
    rejectUnexpectedOptions(options, new Set());
    if (options.positional.length > 0) {
      throw new CliInputError('INVALID_ARGUMENT', 'doctor does not accept positional arguments.');
    }
    return core.getEnvironmentState();
  }

  if (first === 'runtimes' && second === 'list') {
    rejectUnexpectedOptions(options, new Set());
    if (options.positional.length > 0) {
      throw new CliInputError('INVALID_ARGUMENT', 'runtimes list does not accept positional arguments.');
    }
    return core.getEnvironmentState();
  }

  if (first === 'runtime' && second === 'scan') {
    rejectUnexpectedOptions(options, new Set(['family']));
    if (options.positional.length > 0) {
      throw new CliInputError('INVALID_ARGUMENT', 'runtime scan does not accept positional arguments.');
    }
    const family = option(options, 'family');
    return core.scanInstalledRuntimes(family === undefined ? undefined : { runtimeFamily: family });
  }

  if (first === 'runtime' && second === 'verify') {
    rejectUnexpectedOptions(options, new Set(['path', 'family']));
    if (options.positional.length > 0) {
      throw new CliInputError('INVALID_ARGUMENT', 'runtime verify does not accept positional arguments.');
    }
    return core.verifyRuntimePath(requiredOption(options, 'path'), option(options, 'family'));
  }

  if (first === 'catalog' && second === 'list') {
    rejectUnexpectedOptions(options, new Set(['kind', 'family']));
    if (options.positional.length > 0) {
      throw new CliInputError('INVALID_ARGUMENT', 'catalog list does not accept positional arguments.');
    }
    return core.listRuntimeCatalog(aliasedOption(options, 'kind', 'family'));
  }

  if (first === 'runtime' && (second === 'add' || second === 'enable' || second === 'disable')) {
    rejectUnexpectedOptions(options, second === 'add'
      ? new Set(['path', 'scope', 'actor'])
      : new Set(['runtime-id', 'scope', 'actor']));
    if (options.positional.length > 0) {
      throw new CliInputError('INVALID_ARGUMENT', `runtime ${second} does not accept positional arguments.`);
    }
    if (second === 'add') {
      requiredOption(options, 'path');
    } else if (option(options, 'runtime-id') === undefined) {
      throw new CliInputError('INVALID_ARGUMENT', `runtime ${second} requires --runtime-id; register a path first.`);
    }
    const action = second === 'add' ? 'runtime.add' : second;
    return core.planEnvironmentChange(changeInput(options, action));
  }

  if (first === 'change' && second === 'plan') {
    rejectUnexpectedOptions(options, new Set(['action', 'path', 'runtime-id', 'target-runtime-id', 'scope', 'actor']));
    if (options.positional.length > 0) {
      throw new CliInputError('INVALID_ARGUMENT', 'change plan does not accept positional arguments.');
    }
    return core.planEnvironmentChange(changeInput(options));
  }

  if (first === 'change' && second === 'confirm') {
    rejectUnexpectedOptions(options, new Set(['plan-id', 'actor']));
    if (options.positional.length > 0) {
      throw new CliInputError('INVALID_ARGUMENT', 'change confirm does not accept positional arguments.');
    }
    return core.confirmPlan(requiredOption(options, 'plan-id'), option(options, 'actor'));
  }

  if (first === 'change' && second === 'apply') {
    rejectUnexpectedOptions(options, new Set(['plan-id', 'plan', 'plan-hash', 'confirmation-token', 'idempotency-key', 'actor']));
    if (options.positional.length > 0) {
      throw new CliInputError('INVALID_ARGUMENT', 'change apply does not accept positional arguments.');
    }
    const planId = aliasedOption(options, 'plan-id', 'plan');
    if (!planId) {
      throw new CliInputError('INVALID_ARGUMENT', 'Missing required option --plan-id (or --plan).');
    }
    const confirmationToken = option(options, 'confirmation-token');
    if (!confirmationToken) {
      throw new CliInputError('CONFIRMATION_REQUIRED', 'A confirmation token is required before applying a plan.');
    }
    const planHash = option(options, 'plan-hash');
    const idempotencyKey = requiredOption(options, 'idempotency-key');
    const actor = option(options, 'actor');
    return core.applyEnvironmentChange({
      planId,
      confirmationToken,
      idempotencyKey,
      ...(planHash === undefined ? {} : { planHash }),
      ...(actor === undefined ? {} : { actor }),
    });
  }

  if (first === 'operation' && (second === 'get' || second === 'cancel')) {
    rejectUnexpectedOptions(options, new Set(['id']));
    if (options.positional.length > 0) {
      throw new CliInputError('INVALID_ARGUMENT', `operation ${second} does not accept positional arguments.`);
    }
    const operationId = requiredOption(options, 'id');
    return second === 'get' ? core.getOperationStatus(operationId) : core.cancelOperation(operationId);
  }

  throw new CliInputError('UNKNOWN_COMMAND', `Unknown command: ${command.join(' ')}.`);
}

function outputEnvelope(write: (line: string) => void, envelope: CliEnvelope): void {
  write(`${JSON.stringify(envelope)}\n`);
}

function errorExitCode(error: ErrorEnvelope['error']): number {
  if (
    error.code === 'INVALID_ARGUMENT' ||
    error.code === 'INVALID_PARAMS' ||
    error.code === 'UNKNOWN_COMMAND' ||
    error.code === 'INVALID_INPUT' ||
    error.code === 'RUNTIME_NOT_FOUND' ||
    error.code === 'PLAN_NOT_FOUND' ||
    error.code === 'IDEMPOTENCY_REQUIRED'
  ) {
    return 2;
  }
  if (
    error.code === 'CONFIRMATION_REQUIRED' ||
    error.code === 'CONFIRMATION_INVALID' ||
    error.code === 'PLAN_EXPIRED' ||
    error.code === 'PLAN_STALE' ||
    error.code === 'PATH_NOT_ALLOWED' ||
    error.code === 'PATH_NOT_FOUND' ||
    error.code === 'INVALID_RUNTIME_PATH' ||
    error.code === 'PATH_CONFLICT' ||
    error.code === 'PATH_LEASE_CONFLICT' ||
    error.code === 'CONCURRENT_MODIFICATION' ||
    error.code === 'CONFLICT' ||
    error.code === 'IDEMPOTENCY_CONFLICT'
  ) {
    return 3;
  }
  if (
    error.code === 'OPERATION_FAILED' ||
    error.code === 'OPERATION_CANCELLED' ||
    error.code === 'OPERATION_NOT_FOUND' ||
    error.code === 'OPERATION_TIMEOUT' ||
    error.code === 'OPERATION_NOT_CANCELLABLE' ||
    error.code === 'TIMEOUT'
  ) {
    return 4;
  }
  if (
    error.code === 'CATALOG_INVALID' ||
    error.code === 'CATALOG_NOT_FOUND' ||
    error.code === 'CATALOG_EXPIRED' ||
    error.code === 'HASH_MISMATCH' ||
    error.code === 'CHECKSUM_MISMATCH' ||
    error.code === 'SIGNATURE_INVALID' ||
    error.code === 'UNSUPPORTED_ARTIFACT' ||
    error.code === 'UNSUPPORTED_RUNTIME' ||
    error.code === 'UNSUPPORTED_SCOPE' ||
    error.code === 'UNAVAILABLE' ||
    error.code === 'CATALOG_REJECTED'
  ) {
    return 5;
  }
  return 10;
}

export async function runCli(argv: string[], dependencies?: CliDependencies): Promise<number> {
  if (dependencies === undefined) {
    try {
      const core = await loadDefaultCore();
      return runCli(argv, { core });
    } catch (error) {
      const requestId = randomUUID();
      const envelope = errorEnvelope(requestId, error);
      outputEnvelope((line) => process.stdout.write(line), envelope);
      return errorExitCode(envelope.error);
    }
  }
  const requestId = dependencies.requestId?.() ?? randomUUID();
  const write = dependencies.write ?? ((line: string) => process.stdout.write(line));
  const jsonArgv = argv.filter((argument) => argument !== '--json');
  let envelope: CliEnvelope;
  try {
    const options = parseOptions(jsonArgv.slice(2));
    const result = await executeCommand(dependencies.core, jsonArgv.slice(0, 2), options);
    envelope = successEnvelope(requestId, result);
  } catch (error) {
    envelope = errorEnvelope(requestId, error);
  }
  outputEnvelope(write, envelope);
  return envelope.ok ? 0 : errorExitCode(envelope.error);
}

async function loadDefaultCore(): Promise<CliCorePort> {
  // Keep the package name out of static imports so this adapter remains
  // testable before the core workspace package is linked.
  const corePackageName = '@cxszn-menv/core';
  const loaded = (await import(corePackageName)) as {
    createCoreService?: () => CliCorePort;
    CoreService?: new () => CliCorePort;
  };
  if (typeof loaded.createCoreService === 'function') {
    return loaded.createCoreService();
  }
  if (typeof loaded.CoreService === 'function') {
    return new loaded.CoreService();
  }
  throw new Error('Core package does not expose createCoreService or CoreService.');
}

export async function main(argv: string[] = process.argv.slice(2)): Promise<number> {
  if (argv[0] === 'mcp' && argv[1] === 'serve') {
    return runMcpServe(argv);
  }
  return runCli(argv);
}
