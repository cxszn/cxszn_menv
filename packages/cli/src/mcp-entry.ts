import type { Readable, Writable } from 'node:stream';

import type { CliCorePort } from './index.js';

/** Options passed to the MCP stdio adapter. */
export interface McpRunnerOptions {
  service: CliCorePort;
  input?: Readable;
  output?: Writable;
  logger?: (message: string) => void;
}
/** Injectable runner used by the CLI entry point and by tests. */
export type McpServeRunner = (options: McpRunnerOptions) => Promise<void>;

export interface McpServeDependencies {
  /** Core service supplied by the host. Defaults to the workspace core package. */
  core?: CliCorePort;
  input?: Readable;
  output?: Writable;
  logger?: (message: string) => void;
  /** MCP runner supplied by the host. Defaults to @cxszn-menv/mcp/runStdioMcp. */
  runServer?: McpServeRunner;
  loadCore?: () => Promise<CliCorePort>;
}

type ParsedMcpArgs = {
  transport: 'stdio';
};

const INVALID_ARGS_EXIT_CODE = 2;
const STARTUP_FAILURE_EXIT_CODE = 10;

function logError(logger: (message: string) => void, message: string): void {
  logger(message);
}

/**
 * Parse the deliberately narrow MCP CLI surface.
 *
 * The command accepts either the complete argv (`mcp serve ...`) or the
 * subcommand tail (`serve ...`) so embedders can invoke it without rewriting
 * process arguments. Only local stdio is supported in the first release.
 */
function parseMcpArgs(argv: string[]): ParsedMcpArgs {
  const tokens = argv[0] === 'mcp' ? argv.slice(1) : argv.slice();
  if (tokens[0] !== 'serve') {
    throw new Error('Expected the MCP command to be `mcp serve`.');
  }
  tokens.shift();

  let transport: 'stdio' = 'stdio';
  for (let index = 0; index < tokens.length; index += 1) {
    const token = tokens[index];
    if (token === undefined) continue;
    if (token === '--transport') {
      const value = tokens[index + 1];
      if (value === undefined || value.startsWith('--')) {
        throw new Error('--transport requires a value (stdio).');
      }
      if (value !== 'stdio') {
        throw new Error(`Unsupported MCP transport "${value}"; only stdio is allowed.`);
      }
      transport = 'stdio';
      index += 1;
      continue;
    }
    if (token.startsWith('--transport=')) {
      const value = token.slice('--transport='.length);
      if (value !== 'stdio') {
        throw new Error(`Unsupported MCP transport "${value}"; only stdio is allowed.`);
      }
      transport = 'stdio';
      continue;
    }
    if (token.startsWith('-')) {
      throw new Error(`Unknown MCP serve option "${token}".`);
    }
    throw new Error(`Unexpected MCP serve argument "${token}".`);
  }

  return { transport };
}

async function loadDefaultCore(): Promise<CliCorePort> {
  // Keep the package name out of static imports so the CLI remains testable
  // before the workspace dependency is linked.
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

async function loadDefaultRunner(): Promise<McpServeRunner> {
  const mcpPackageName = '@cxszn-menv/mcp';
  const loaded = (await import(mcpPackageName)) as {
    runStdioMcp?: (options: McpRunnerOptions) => Promise<void>;
  };
  if (typeof loaded.runStdioMcp !== 'function') {
    throw new Error('MCP package does not expose runStdioMcp.');
  }
  return loaded.runStdioMcp;
}

/**
 * Run `menv mcp serve --transport stdio`.
 *
 * This is intentionally a thin adapter: it validates transport/flags, wires
 * the shared core service to the MCP stdio runner, and never evaluates shell
 * commands or accepts network transports.
 */
export async function runMcpServe(
  argv: string[],
  dependencies: McpServeDependencies = {},
): Promise<number> {
  const logger = dependencies.logger ?? ((message: string) => process.stderr.write(`${message}\n`));
  let options: ParsedMcpArgs;
  try {
    options = parseMcpArgs(argv);
  } catch (error) {
    logError(logger, error instanceof Error ? error.message : String(error));
    return INVALID_ARGS_EXIT_CODE;
  }

  // Keep this assignment visible to make it explicit that transport validation
  // is part of the public contract even while stdio is the only value.
  if (options.transport !== 'stdio') {
    logError(logger, 'Only the local stdio MCP transport is supported.');
    return INVALID_ARGS_EXIT_CODE;
  }

  try {
    const core = dependencies.core ?? await (dependencies.loadCore ?? loadDefaultCore)();
    const runServer = dependencies.runServer ?? await loadDefaultRunner();
    const runnerOptions: McpRunnerOptions = {
      service: core,
      ...(dependencies.input === undefined ? {} : { input: dependencies.input }),
      ...(dependencies.output === undefined ? {} : { output: dependencies.output }),
      ...(dependencies.logger === undefined ? {} : { logger: dependencies.logger }),
    };
    await runServer(runnerOptions);
    return 0;
  } catch (error) {
    // Avoid printing arbitrary exception payloads (which could contain a
    // command line or environment value); the adapter only reports a concise
    // startup failure and leaves detailed diagnostics to the MCP logger.
    const message = error instanceof Error ? error.message : 'Unknown MCP startup failure.';
    logError(logger, `Unable to start MCP stdio server: ${message}`);
    return STARTUP_FAILURE_EXIT_CODE;
  }
}
