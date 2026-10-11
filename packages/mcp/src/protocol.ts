/**
 * Public protocol types shared by the MCP adapter and its callers.
 *
 * The adapter deliberately keeps these types JSON-shaped.  The core package
 * can therefore be replaced by the Rust/Tauri host later without making the
 * wire protocol depend on Node, Tauri, or Windows-specific types.
 */

export type JsonPrimitive = string | number | boolean | null;
export type JsonValue = JsonPrimitive | JsonObject | JsonValue[];
export interface JsonObject {
  [key: string]: JsonValue | undefined;
}
export type JsonRpcId = string | number | null;

export interface JsonRpcRequest {
  jsonrpc: "2.0";
  id?: JsonRpcId;
  method: string;
  params?: JsonValue;
}

export interface JsonRpcError {
  code: number;
  message: string;
  data?: JsonValue;
}

export interface JsonRpcResponse {
  jsonrpc: "2.0";
  id: JsonRpcId;
  result?: JsonValue;
  error?: JsonRpcError;
}

export interface MenvError {
  code: string;
  message: string;
  retryable: boolean;
  next_actions: string[];
  details?: JsonValue;
}

export interface MenvSuccess<T extends JsonValue = JsonValue> {
  schema_version: "1";
  request_id: string;
  ok: true;
  result: T;
  operation_id?: string;
}

export interface MenvFailure {
  schema_version: "1";
  request_id: string;
  ok: false;
  error: MenvError;
}

export type MenvEnvelope<T extends JsonValue = JsonValue> = MenvSuccess<T> | MenvFailure;

/**
 * The smallest port needed by MCP.  Implementations may be the TypeScript
 * CoreService or a future host adapter; no business logic is duplicated here.
 */
export interface CoreService {
  getEnvironmentState(input?: unknown): Promise<unknown> | unknown;
  scanInstalledRuntimes(input?: unknown): Promise<unknown> | unknown;
  listRuntimeCatalog(kind?: string): Promise<unknown> | unknown;
  verifyRuntimePath(path: string, family?: string): Promise<unknown> | unknown;
  planEnvironmentChange(input: unknown): Promise<unknown> | unknown;
  applyEnvironmentChange(input: ApplyEnvironmentChangeInput): Promise<unknown> | unknown;
  getOperationStatus(operationId: string): Promise<unknown> | unknown;
  cancelOperation(operationId: string): Promise<unknown> | unknown;
}

export interface ApplyEnvironmentChangeInput {
  planId: string;
  planHash?: string;
  confirmationToken: string;
  idempotencyKey: string;
  actor?: string;
}

export interface McpContentBlock {
  type: "text";
  text: string;
}

export interface McpToolResult {
  content: McpContentBlock[];
  structuredContent: MenvEnvelope;
  isError?: boolean;
}

export interface McpToolDefinition {
  name: string;
  description: string;
  inputSchema: {
    type: "object";
    properties?: Record<string, JsonValue>;
    required?: string[];
    additionalProperties: false;
  };
}

export interface McpServerOptions {
  service: CoreService;
  serverInfo?: {
    name: string;
    version: string;
  };
  protocolVersion?: string;
  logger?: (message: string) => void;
}

export const MCP_PROTOCOL_VERSION = "2025-11-25";
export const DEFAULT_SERVER_INFO = Object.freeze({ name: "cxszn-menv", version: "0.0.1" });

export const MCP_TOOL_DEFINITIONS: readonly McpToolDefinition[] = Object.freeze([
  {
    name: "menv_get_environment_state",
    description: "Read the current MEnv runtime, activation, lease, and operation state.",
    inputSchema: { type: "object", properties: {}, additionalProperties: false },
  },
  {
    name: "menv_scan_installed_runtimes",
    description: "Scan installed runtimes without registering or changing them.",
    inputSchema: {
      type: "object",
      properties: { runtime_family: { type: "string", enum: ["php", "node", "mysql", "python"] } },
      additionalProperties: false,
    },
  },
  {
    name: "menv_list_runtime_catalog",
    description: "List verified runtime catalog entries, optionally filtered by family.",
    inputSchema: {
      type: "object",
      properties: {
        kind: { type: "string" },
        runtime_family: { type: "string", enum: ["php", "node", "mysql", "python"] },
        include_archival: { type: "boolean" },
      },
      additionalProperties: false,
    },
  },
  {
    name: "menv_verify_runtime_path",
    description: "Read-only validation of an explicitly provided runtime executable path.",
    inputSchema: {
      type: "object",
      properties: {
        path: { type: "string", minLength: 1 },
        runtime_family: { type: "string", enum: ["php", "node", "mysql", "python"] },
      },
      required: ["path"],
      additionalProperties: false,
    },
  },
  {
    name: "menv_plan_environment_change",
    description: "Create a dry-run plan. Planning does not install, enable, or modify PATH.",
    inputSchema: {
      type: "object",
      properties: {
        action: {
          type: "string",
          enum: [
            "register_path",
            "enable",
            "disable",
            "switch",
            "runtime.add",
          ],
        },
        runtime_id: { type: "string" },
        path: { type: "string" },
        target_runtime_id: { type: "string" },
        scope: { type: "string", enum: ["user", "process"] },
        change: {
          type: "object",
          properties: {
            action: {
              type: "string",
              enum: ["register_path", "enable", "disable", "switch", "runtime.add"],
            },
            runtime_id: { type: "string" },
            path: { type: "string", minLength: 1 },
            target_runtime_id: { type: "string" },
            scope: { type: "string", enum: ["user", "process"] },
          },
          additionalProperties: false,
        },
      },
      additionalProperties: false,
    },
  },
  {
    name: "menv_apply_environment_change",
    description: "Apply an approved plan using a short-lived confirmation token and idempotency key.",
    inputSchema: {
      type: "object",
      properties: {
        plan_id: { type: "string", minLength: 1 },
        plan_hash: { type: "string" },
        confirmation_token: { type: "string", minLength: 1 },
        idempotency_key: { type: "string", minLength: 1 },
        actor: { type: "string" },
      },
      required: ["plan_id", "confirmation_token", "idempotency_key"],
      additionalProperties: false,
    },
  },
  {
    name: "menv_get_operation_status",
    description: "Read an operation status by operation id.",
    inputSchema: {
      type: "object",
      properties: { operation_id: { type: "string", minLength: 1 } },
      required: ["operation_id"],
      additionalProperties: false,
    },
  },
  {
    name: "menv_cancel_operation",
    description: "Cancel an operation that is still cancellable.",
    inputSchema: {
      type: "object",
      properties: {
        operation_id: { type: "string", minLength: 1 },
        idempotency_key: { type: "string", minLength: 1 },
      },
      required: ["operation_id"],
      additionalProperties: false,
    },
  },
]);

export const SUPPORTED_TOOL_NAMES = new Set(MCP_TOOL_DEFINITIONS.map((tool) => tool.name));

/** Tool names which must never be made available through this adapter. */
export const FORBIDDEN_TOOL_NAMES = new Set([
  "run_shell",
  "run_powershell",
  "execute_command",
  "execute_sql",
  "download_url",
  "download",
  "delete_file",
  "remove_file",
  "file_delete",
]);

export function isJsonValue(value: unknown): value is JsonValue {
  if (value === null || typeof value === "string" || typeof value === "number" || typeof value === "boolean") {
    return true;
  }
  if (Array.isArray(value)) return value.every(isJsonValue);
  if (typeof value !== "object") return false;
  return Object.values(value as Record<string, unknown>).every((item) => item === undefined || isJsonValue(item));
}
