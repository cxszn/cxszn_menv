import { createInterface } from "node:readline";
import type { Readable, Writable } from "node:stream";
import {
  DEFAULT_SERVER_INFO,
  FORBIDDEN_TOOL_NAMES,
  MCP_PROTOCOL_VERSION,
  MCP_TOOL_DEFINITIONS,
  SUPPORTED_TOOL_NAMES,
  type ApplyEnvironmentChangeInput,
  type CoreService,
  type JsonObject,
  type JsonRpcId,
  type JsonRpcRequest,
  type JsonRpcResponse,
  type JsonValue,
  type McpServerOptions,
  type McpToolResult,
  type MenvEnvelope,
  type MenvError,
  isJsonValue,
} from "./protocol.js";

const JSON_RPC_PARSE_ERROR = -32700;
const JSON_RPC_INVALID_REQUEST = -32600;
const JSON_RPC_METHOD_NOT_FOUND = -32601;
const JSON_RPC_INVALID_PARAMS = -32602;
const JSON_RPC_INTERNAL_ERROR = -32603;

type RecordValue = Record<string, unknown>;

const RUNTIME_FAMILIES = new Set(["php", "node", "mysql", "python"]);
// Process/session environment injection belongs to the host-only ConPTY
// surface. It is not exposed through MCP until that adapter has real evidence.
const ENVIRONMENT_SCOPES = new Set(["user"]);

const hasOwn = (value: object, key: PropertyKey): boolean => Object.prototype.hasOwnProperty.call(value, key);

function isRecord(value: unknown): value is RecordValue {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
function asJsonValue(value: unknown): JsonValue {
  if (value === undefined) return null;
  if (isJsonValue(value)) return value;
  // Core methods are expected to return JSON-shaped values.  If a future
  // implementation accidentally leaks a class/object, fail closed rather
  // than serializing non-deterministic properties into the protocol.
  return { value: String(value) };
}

function requestId(id: JsonRpcId | undefined): string {
  return id === undefined || id === null ? "mcp-request" : String(id);
}

function isForbiddenToolName(name: string): boolean {
  const normalized = name.trim().toLowerCase();
  return FORBIDDEN_TOOL_NAMES.has(normalized)
    || normalized.includes("shell")
    || normalized.includes("powershell")
    || normalized.includes("sql")
    || normalized.includes("delete")
    || normalized.includes("remove-file")
    || normalized.includes("remove_file")
    || normalized.includes("download");
}

function rpcError(id: JsonRpcId, code: number, message: string, data?: JsonValue): JsonRpcResponse {
  return {
    jsonrpc: "2.0",
    id,
    error: { code, message, ...(data === undefined ? {} : { data }) },
  };
}

function jsonRpcResponse(id: JsonRpcId, result: JsonValue): JsonRpcResponse {
  return { jsonrpc: "2.0", id, result };
}

function errorCode(error: unknown): string {
  if (isRecord(error) && typeof error.code === "string" && error.code.length > 0) return error.code;
  return "INTERNAL_ERROR";
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (isRecord(error) && typeof error.message === "string" && error.message.length > 0) return error.message;
  return "The core service failed to complete the request.";
}

function errorRetryable(error: unknown): boolean {
  return isRecord(error) && typeof error.retryable === "boolean" ? error.retryable : false;
}

function errorNextActions(error: unknown): string[] {
  if (!isRecord(error)) return [];
  const actions = Array.isArray(error.next_actions)
    ? error.next_actions
    : Array.isArray(error.nextActions)
      ? error.nextActions
      : [];
  return actions.filter((action): action is string => typeof action === "string");
}

function errorDetails(error: unknown): JsonValue | undefined {
  if (!isRecord(error) || error.details === undefined || !isJsonValue(error.details)) return undefined;
  return error.details;
}

function makeFailure(
  id: string,
  code: string,
  message: string,
  retryable = false,
  details?: JsonValue,
  nextActions: string[] = [],
): MenvEnvelope {
  const error: MenvError = {
    code,
    message,
    retryable,
    next_actions: nextActions,
    ...(details === undefined ? {} : { details }),
  };
  return { schema_version: "1", request_id: id, ok: false, error };
}

function makeSuccess(id: string, value: unknown): MenvEnvelope {
  const result = asJsonValue(value);
  const operationId = isRecord(value) && typeof value.operationId === "string"
    ? value.operationId
    : isRecord(value) && typeof value.operation_id === "string"
      ? value.operation_id
      : undefined;
  return {
    schema_version: "1",
    request_id: id,
    ok: true,
    result,
    ...(operationId === undefined ? {} : { operation_id: operationId }),
  };
}

function makeToolResult(envelope: MenvEnvelope): McpToolResult {
  return {
    content: [{ type: "text", text: JSON.stringify(envelope) }],
    structuredContent: envelope,
    ...(envelope.ok ? {} : { isError: true }),
  };
}

function requireObject(value: unknown, label: string): RecordValue {
  if (!isRecord(value)) throw new InputError(`${label} must be an object.`);
  return value;
}

function rejectUnknownFields(value: RecordValue, allowed: readonly string[], label: string): void {
  const unknown = Object.keys(value).filter((key) => !allowed.includes(key));
  if (unknown.length > 0) {
    throw new InputError(`${label} contains unsupported field(s): ${unknown.join(", ")}.`);
  }
}

function requireString(value: unknown, label: string): string {
  if (typeof value !== "string" || value.trim().length === 0) throw new InputError(`${label} must be a non-empty string.`);
  return value;
}

function optionalString(value: unknown, label: string): string | undefined {
  if (value === undefined) return undefined;
  return requireString(value, label);
}

function optionalRuntimeFamily(value: unknown, label: string): string | undefined {
  const family = optionalString(value, label);
  if (family !== undefined && !RUNTIME_FAMILIES.has(family)) {
    throw new InputError(`${label} must be one of: ${[...RUNTIME_FAMILIES].join(", ")}.`);
  }
  return family;
}

function optionalScope(value: unknown, label: string): string | undefined {
  const scope = optionalString(value, label);
  if (scope !== undefined && !ENVIRONMENT_SCOPES.has(scope)) {
    throw new InputError(`${label} must be one of: ${[...ENVIRONMENT_SCOPES].join(", ")}.`);
  }
  return scope;
}

function inputErrorEnvelope(id: string, error: unknown): MenvEnvelope {
  return makeFailure(id, "INVALID_PARAMS", errorMessage(error), false);
}

class InputError extends Error {
  public constructor(message: string) {
    super(message);
    this.name = "InputError";
  }
}

export interface McpServer {
  /** Handle an already parsed JSON-RPC message. */
  handleMessage(message: unknown): Promise<JsonRpcResponse | undefined>;
  /** Parse and handle one newline-delimited JSON-RPC message. */
  handleLine(line: string): Promise<string | undefined>;
}

export function createMcpServer(options: McpServerOptions): McpServer {
  const service = options.service;
  const protocolVersion = options.protocolVersion ?? MCP_PROTOCOL_VERSION;
  const serverInfo = options.serverInfo ?? DEFAULT_SERVER_INFO;
  const logger = options.logger ?? (() => undefined);

  async function invokeTool(name: string, args: RecordValue, id: string): Promise<McpToolResult> {
    if (isForbiddenToolName(name)) {
      return makeToolResult(makeFailure(
        id,
        "FORBIDDEN_TOOL",
        "Shell execution, URL downloads, SQL, file deletion, and implicit downloads are not exposed by MEnv MCP.",
      ));
    }
    if (!SUPPORTED_TOOL_NAMES.has(name)) {
      return makeToolResult(makeFailure(id, "UNKNOWN_TOOL", `Unknown tool: ${name}.`));
    }

    try {
      let value: unknown;
      switch (name) {
        case "menv_get_environment_state":
          rejectUnknownFields(args, [], name);
          value = await service.getEnvironmentState();
          break;
        case "menv_scan_installed_runtimes":
          rejectUnknownFields(args, ["runtime_family"], name);
          {
            const runtimeFamily = optionalRuntimeFamily(args.runtime_family, "runtime_family");
            value = runtimeFamily === undefined
              ? await service.scanInstalledRuntimes()
              : await service.scanInstalledRuntimes({ runtimeFamily });
          }
          break;
        case "menv_list_runtime_catalog": {
          rejectUnknownFields(args, ["kind", "runtime_family", "include_archival"], name);
          const requestedKind = optionalString(args.kind, "kind");
          const requestedFamily = optionalRuntimeFamily(args.runtime_family, "runtime_family");
          if (requestedKind !== undefined && requestedFamily !== undefined && requestedKind !== requestedFamily) {
            throw new InputError("kind and runtime_family must match when both are provided.");
          }
          const kind = requestedKind ?? requestedFamily;
          if (args.include_archival !== undefined && typeof args.include_archival !== "boolean") {
            throw new InputError("include_archival must be a boolean.");
          }
          const includeArchival = args.include_archival === true;
          // Catalog metadata is intentionally non-downloadable over MCP.  A
          // core implementation may retain a source URL for its installer,
          // but exposing it here would create an implicit download escape
          // hatch for agents.
          value = stripCatalogUrls(filterArchival(await service.listRuntimeCatalog(kind), includeArchival));
          break;
        }
        case "menv_verify_runtime_path": {
          rejectUnknownFields(args, ["path", "runtime_family"], name);
          const runtimeFamily = optionalRuntimeFamily(args.runtime_family, "runtime_family");
          value = await service.verifyRuntimePath(requireString(args.path, "path"), runtimeFamily);
          break;
        }
        case "menv_plan_environment_change": {
          rejectUnknownFields(args, [
            "action",
            "runtime_id",
            "path",
            "target_runtime_id",
            "scope",
            "change",
          ], name);
          const change = makePlanInput(args);
          value = await service.planEnvironmentChange(change);
          break;
        }
        case "menv_apply_environment_change": {
          rejectUnknownFields(args, ["plan_id", "plan_hash", "confirmation_token", "idempotency_key", "actor"], name);
          const planHash = optionalString(args.plan_hash, "plan_hash");
          const actor = optionalString(args.actor, "actor");
          const input: ApplyEnvironmentChangeInput = {
            planId: requireString(args.plan_id, "plan_id"),
            confirmationToken: requireString(args.confirmation_token, "confirmation_token"),
            idempotencyKey: requireString(args.idempotency_key, "idempotency_key"),
            ...(planHash === undefined ? {} : { planHash }),
            ...(actor === undefined ? {} : { actor }),
          };
          value = await service.applyEnvironmentChange(input);
          break;
        }
        case "menv_get_operation_status": {
          rejectUnknownFields(args, ["operation_id"], name);
          value = await service.getOperationStatus(requireString(args.operation_id, "operation_id"));
          break;
        }
        case "menv_cancel_operation": {
          rejectUnknownFields(args, ["operation_id", "idempotency_key"], name);
          // CoreService currently owns cancellation by operation id.  An
          // optional idempotency key is still accepted for callers that retry
          // through a transport which requires a key on every write.
          optionalString(args.idempotency_key, "idempotency_key");
          value = await service.cancelOperation(requireString(args.operation_id, "operation_id"));
          break;
        }
        default:
          return makeToolResult(makeFailure(id, "UNKNOWN_TOOL", `Unknown tool: ${name}.`));
      }
      return makeToolResult(makeSuccess(id, value));
    } catch (error) {
      if (error instanceof InputError) return makeToolResult(inputErrorEnvelope(id, error));
      logger(`MCP core call ${name} failed: ${errorMessage(error)}`);
      return makeToolResult(makeFailure(
        id,
        errorCode(error),
        errorMessage(error),
        errorRetryable(error),
        errorDetails(error),
        errorNextActions(error),
      ));
    }
  }

  async function handleRequest(request: JsonRpcRequest): Promise<JsonRpcResponse | undefined> {
    const hasId = hasOwn(request, "id");
    const id = hasId ? request.id ?? null : null;
    const requestIdValue = requestId(request.id);

    switch (request.method) {
      case "initialize":
        if (!hasId) return undefined;
        return jsonRpcResponse(id, {
          protocolVersion,
          capabilities: { tools: {} },
          serverInfo,
        });
      case "notifications/initialized":
        return undefined;
      case "ping":
        return hasId ? jsonRpcResponse(id, {}) : undefined;
      case "tools/list": {
        if (!hasId) return undefined;
        try {
          const params = request.params === undefined ? {} : requireObject(request.params, "params");
          rejectUnknownFields(params, [], "tools/list params");
        } catch (error) {
          return rpcError(id, JSON_RPC_INVALID_PARAMS, errorMessage(error));
        }
        return jsonRpcResponse(id, { tools: [...MCP_TOOL_DEFINITIONS] } as unknown as JsonValue);
      }
      case "tools/call": {
        if (!hasId) return undefined;
        try {
          const params = requireObject(request.params, "tools/call params");
          rejectUnknownFields(params, ["name", "arguments"], "tools/call params");
          const name = requireString(params.name, "name");
          const args = params.arguments === undefined ? {} : requireObject(params.arguments, "arguments");
          const result = await invokeTool(name, args, requestIdValue);
          return jsonRpcResponse(id, result as unknown as JsonValue);
        } catch (error) {
          return rpcError(id, JSON_RPC_INVALID_PARAMS, errorMessage(error));
        }
      }
      default:
        return hasId ? rpcError(id, JSON_RPC_METHOD_NOT_FOUND, `Method not found: ${request.method}.`) : undefined;
    }
  }

  return {
    async handleMessage(message: unknown): Promise<JsonRpcResponse | undefined> {
      if (!isRecord(message) || message.jsonrpc !== "2.0" || typeof message.method !== "string") {
        return rpcError(null, JSON_RPC_INVALID_REQUEST, "Invalid JSON-RPC request.");
      }
      const rawId = message.id;
      if (rawId !== undefined && rawId !== null && typeof rawId !== "string" && typeof rawId !== "number") {
        return rpcError(null, JSON_RPC_INVALID_REQUEST, "JSON-RPC id must be a string, number, or null.");
      }
      if (message.params !== undefined && !isJsonValue(message.params)) {
        return rpcError((rawId as JsonRpcId | undefined) ?? null, JSON_RPC_INVALID_REQUEST, "params must be JSON.");
      }
      return handleRequest(message as unknown as JsonRpcRequest);
    },

    async handleLine(line: string): Promise<string | undefined> {
      if (line.trim().length === 0) return undefined;
      let message: unknown;
      try {
        message = JSON.parse(line) as unknown;
      } catch {
        return JSON.stringify(rpcError(null, JSON_RPC_PARSE_ERROR, "Parse error."));
      }
      const response = await this.handleMessage(message);
      return response === undefined ? undefined : JSON.stringify(response);
    },
  };
}

function makePlanInput(args: RecordValue): JsonObject {
  if (args.change !== undefined) {
    const topLevelFields = Object.keys(args).filter((key) => key !== "change");
    if (topLevelFields.length > 0) {
      throw new InputError(`change cannot be combined with top-level field(s): ${topLevelFields.join(", ")}.`);
    }
    const change = requireObject(args.change, "change");
    rejectUnknownFields(change, [
      "action",
      "runtime_id",
      "path",
      "target_runtime_id",
      "scope",
    ], "change");
    const result: JsonObject = {};
    for (const [key, value] of Object.entries(change)) {
      if (value !== undefined && !isJsonValue(value)) throw new InputError(`change.${key} must be JSON.`);
      if (key === "runtime_id") result.runtimeId = value as JsonValue | undefined;
      else if (key === "target_runtime_id") result.targetRuntimeId = value as JsonValue | undefined;
      else result[key] = value as JsonValue | undefined;
    }
    const scope = optionalScope(result.scope, "change.scope");
    if (scope !== undefined) result.scope = scope;
    validatePlanAction(result.action, result);
    return result;
  }

  const action = requireString(args.action, "action");
  const runtimeId = optionalString(args.runtime_id, "runtime_id");
  const path = optionalString(args.path, "path");
  const targetRuntimeId = optionalString(args.target_runtime_id, "target_runtime_id");
  const scope = optionalScope(args.scope, "scope");
  const result: JsonObject = {
    action,
    ...(runtimeId === undefined ? {} : { runtimeId }),
    ...(path === undefined ? {} : { path }),
    ...(targetRuntimeId === undefined ? {} : { targetRuntimeId }),
    ...(scope === undefined ? {} : { scope }),
  };
  validatePlanAction(action, result);
  return result;
}

function stripCatalogUrls(value: unknown): unknown {
  if (Array.isArray(value)) return value.map((item) => stripCatalogUrls(item));
  if (!isRecord(value)) return value;
  const result: RecordValue = {};
  for (const [key, item] of Object.entries(value)) {
    const normalized = key.toLowerCase();
    if (normalized === "url" || normalized === "downloadurl" || normalized === "download_url") continue;
    result[key] = stripCatalogUrls(item);
  }
  return result;
}

function filterArchival(value: unknown, includeArchival: boolean): unknown {
  if (includeArchival) return value;
  if (Array.isArray(value)) {
    return value.filter((entry) => {
      if (!isRecord(entry)) return true;
      return entry.legacy !== true && entry.archival !== true && entry.eol !== true;
    });
  }
  if (!isRecord(value) || !Array.isArray(value.entries)) return value;
  return {
    ...value,
    entries: value.entries.filter((entry) => {
      if (!isRecord(entry)) return true;
      return entry.legacy !== true && entry.archival !== true && entry.eol !== true;
    }),
  };
}

const PLAN_ACTIONS = new Set([
  "register_path",
  "enable",
  "disable",
  "switch",
  "runtime.add",
]);

function validatePlanAction(actionValue: JsonValue | undefined, input: JsonObject): void {
  const rawAction = requireString(actionValue, "action");
  if (!PLAN_ACTIONS.has(rawAction)) {
    throw new InputError(`action must be one of: ${[...PLAN_ACTIONS].join(", ")}.`);
  }
  // Keep the historical register_path spelling at the MCP boundary while
  // using the single core action name internally.
  const action = rawAction === "register_path" ? "runtime.add" : rawAction;
  input.action = action;
  const path = input.path;
  const runtimeId = input.runtimeId;
  const targetRuntimeId = input.targetRuntimeId;
  if (action === "runtime.add" && (typeof path !== "string" || path.length === 0)) {
    throw new InputError(`${action} requires path.`);
  }
  if (action !== "runtime.add" && path !== undefined) {
    throw new InputError(`${action} does not accept path; register the runtime first.`);
  }
  if ((action === "enable" || action === "disable") && typeof runtimeId !== "string") {
    throw new InputError(`${action} requires runtime_id; register a path before enabling it.`);
  }
  if (action === "switch"
    && (typeof runtimeId !== "string" || typeof targetRuntimeId !== "string")) {
    throw new InputError("switch requires runtime_id and target_runtime_id.");
  }
}

export interface StdioMcpOptions extends McpServerOptions {
  input?: Readable;
  output?: Writable;
}

/** Serve newline-delimited JSON-RPC over local stdio; logs never go to stdout. */
export async function runStdioMcp(options: StdioMcpOptions): Promise<void> {
  const input = options.input ?? process.stdin;
  const output = options.output ?? process.stdout;
  const logger = options.logger ?? ((message: string) => process.stderr.write(`${message}\n`));
  const server = createMcpServer({ ...options, logger });
  const lines = createInterface({ input, crlfDelay: Infinity });

  for await (const line of lines) {
    try {
      const response = await server.handleLine(line);
      if (response !== undefined) output.write(`${response}\n`);
    } catch (error) {
      logger(`MCP request failed: ${errorMessage(error)}`);
      output.write(`${JSON.stringify(rpcError(null, JSON_RPC_INTERNAL_ERROR, "Internal MCP error."))}\n`);
    }
  }
}
