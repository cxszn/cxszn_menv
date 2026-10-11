import { PassThrough } from "node:stream";
import { describe, expect, it } from "vitest";
import {
  createMcpServer,
  type CoreService,
  type MenvEnvelope,
  runStdioMcp,
} from "../src/index.js";

function request(id: number | string, method: string, params?: unknown): string {
  return JSON.stringify({ jsonrpc: "2.0", id, method, ...(params === undefined ? {} : { params }) });
}

function notification(method: string, params?: unknown): string {
  return JSON.stringify({ jsonrpc: "2.0", method, ...(params === undefined ? {} : { params }) });
}

function parseLines(output: string): unknown[] {
  return output
    .trim()
    .split(/\r?\n/)
    .filter(Boolean)
    .map((line) => JSON.parse(line));
}

function testService(): CoreService & { calls: string[] } {
  const calls: string[] = [];
  return {
    calls,
    getEnvironmentState: async () => {
      calls.push("getEnvironmentState");
      return { runtimes: [], active: null };
    },
    scanInstalledRuntimes: async () => {
      calls.push("scanInstalledRuntimes");
      return { runtimes: [] };
    },
    listRuntimeCatalog: async (kind) => {
      calls.push(`listRuntimeCatalog:${kind ?? "all"}`);
      return {
        entries: [
          { artifact_id: "fixture", url: "https://example.invalid/runtime.zip" },
          { artifact_id: "legacy", legacy: true },
        ],
      };
    },
    verifyRuntimePath: async (path) => {
      calls.push(`verifyRuntimePath:${path}`);
      return { path, valid: true };
    },
    planEnvironmentChange: async (input) => {
      calls.push("planEnvironmentChange");
      return { plan_id: "plan-1", plan_hash: "hash-1", summary: input };
    },
    applyEnvironmentChange: async (input) => {
      calls.push("applyEnvironmentChange");
      return { operation_id: "op-1", status: "queued", request: input };
    },
    getOperationStatus: async (operationId) => {
      calls.push(`getOperationStatus:${operationId}`);
      return { operation_id: operationId, status: "succeeded" };
    },
    cancelOperation: async (operationId) => {
      calls.push(`cancelOperation:${operationId}`);
      return { operation_id: operationId, status: "cancelled" };
    },
  };
}

describe("MCP JSON-RPC server", () => {
  it("negotiates initialize and exposes only the supported tools", async () => {
    const server = createMcpServer({ service: testService() });

    const response = await server.handleLine(request(1, "initialize", {
      protocolVersion: "2025-11-25",
      capabilities: {},
      clientInfo: { name: "test", version: "0.0.2" },
    }));
    const parsed = JSON.parse(response ?? "null") as { result: { protocolVersion: string; capabilities: { tools: object }; serverInfo: object } };

    expect(parsed.result.protocolVersion).toBe("2025-11-25");
    expect(parsed.result.capabilities.tools).toEqual({});
    expect(parsed.result.serverInfo).toMatchObject({ name: "cxszn-menv", version: "0.0.2" });

    const toolsResponse = await server.handleLine(request(2, "tools/list"));
    const tools = (JSON.parse(toolsResponse ?? "null") as { result: { tools: Array<{ name: string }> } }).result.tools;
    expect(tools.map((tool) => tool.name)).toEqual([
      "menv_get_environment_state",
      "menv_scan_installed_runtimes",
      "menv_list_runtime_catalog",
      "menv_verify_runtime_path",
      "menv_plan_environment_change",
      "menv_apply_environment_change",
      "menv_get_operation_status",
      "menv_cancel_operation",
    ]);
  });

  it("returns a structured envelope and calls core for read tools", async () => {
    const service = testService();
    const server = createMcpServer({ service });

    const response = await server.handleLine(request("read-1", "tools/call", {
      name: "menv_get_environment_state",
      arguments: {},
    }));
    const parsed = JSON.parse(response ?? "null") as {
      result: { content: Array<{ type: string; text: string }>; structuredContent: MenvEnvelope };
    };

    expect(service.calls).toContain("getEnvironmentState");
    expect(parsed.result.structuredContent).toMatchObject({
      schema_version: "1",
      request_id: "read-1",
      ok: true,
      result: { runtimes: [] },
    });
    expect(parsed.result.content[0]).toMatchObject({ type: "text" });
    expect(JSON.parse(parsed.result.content[0]?.text ?? "null")).toMatchObject({ ok: true });
  });

  it("accepts canonical runtime-family and catalog filters without widening the tool surface", async () => {
    const service = testService();
    const server = createMcpServer({ service });

    const scanResponse = await server.handleLine(request("scan-php", "tools/call", {
      name: "menv_scan_installed_runtimes",
      arguments: { runtime_family: "php" },
    }));
    const catalogResponse = await server.handleLine(request("catalog-php", "tools/call", {
      name: "menv_list_runtime_catalog",
      arguments: { runtime_family: "php", include_archival: true },
    }));
    const supportedOnlyResponse = await server.handleLine(request("catalog-supported", "tools/call", {
      name: "menv_list_runtime_catalog",
      arguments: { runtime_family: "php" },
    }));

    expect(JSON.parse(scanResponse ?? "null").result.structuredContent.ok).toBe(true);
    expect(JSON.parse(catalogResponse ?? "null").result.structuredContent.ok).toBe(true);
    expect(JSON.parse(catalogResponse ?? "null").result.structuredContent.result).toEqual({
      entries: [{ artifact_id: "fixture" }, { artifact_id: "legacy", legacy: true }],
    });
    expect(JSON.parse(supportedOnlyResponse ?? "null").result.structuredContent.result).toEqual({
      entries: [{ artifact_id: "fixture" }],
    });
    expect(service.calls).toContain("listRuntimeCatalog:php");

    const conflictingFilterResponse = await server.handleLine(request("catalog-conflict", "tools/call", {
      name: "menv_list_runtime_catalog",
      arguments: { kind: "php", runtime_family: "node" },
    }));
    expect(JSON.parse(conflictingFilterResponse ?? "null").result.structuredContent).toMatchObject({
      ok: false,
      error: { code: "INVALID_PARAMS" },
    });
  });

  it("requires confirmation and idempotency fields for apply", async () => {
    const service = testService();
    const server = createMcpServer({ service });

    const response = await server.handleLine(request(3, "tools/call", {
      name: "menv_apply_environment_change",
      arguments: { plan_id: "plan-1" },
    }));
    const parsed = JSON.parse(response ?? "null") as {
      result: { isError: boolean; structuredContent: MenvEnvelope };
    };

    expect(parsed.result.isError).toBe(true);
    expect(parsed.result.structuredContent).toMatchObject({
      ok: false,
      error: { code: "INVALID_PARAMS" },
    });
    expect(service.calls).not.toContain("applyEnvironmentChange");
  });

  it("rejects forbidden shell, URL, SQL and file-delete tool names", async () => {
    const server = createMcpServer({ service: testService() });
    for (const name of ["run_shell", "run_powershell", "download_url", "execute_sql", "delete_file"]) {
      const response = await server.handleLine(request(name, "tools/call", { name, arguments: {} }));
      const parsed = JSON.parse(response ?? "null") as {
        result: { isError: boolean; structuredContent: MenvEnvelope };
      };
      expect(parsed.result.isError).toBe(true);
      expect(parsed.result.structuredContent).toMatchObject({ ok: false, error: { code: "FORBIDDEN_TOOL" } });
    }
  });

  it("allows cancellation without an idempotency key while still validating the operation id", async () => {
    const service = testService();
    const server = createMcpServer({ service });
    const response = await server.handleLine(request("cancel-1", "tools/call", {
      name: "menv_cancel_operation",
      arguments: { operation_id: "op-1" },
    }));
    const parsed = JSON.parse(response ?? "null");

    expect(parsed.result.structuredContent).toMatchObject({ ok: true, result: { status: "cancelled" } });
    expect(service.calls).toContain("cancelOperation:op-1");
  });

  it("rejects unsupported plan actions before invoking core", async () => {
    const service = testService();
    const server = createMcpServer({ service });
    const response = await server.handleLine(request("bad-plan", "tools/call", {
      name: "menv_plan_environment_change",
      arguments: { action: "run_shell", runtime_id: "php-8.3" },
    }));
    const parsed = JSON.parse(response ?? "null");

    expect(parsed.result.isError).toBe(true);
    expect(parsed.result.structuredContent).toMatchObject({ ok: false, error: { code: "INVALID_PARAMS" } });
    expect(service.calls).not.toContain("planEnvironmentChange");

    const installResponse = await server.handleLine(request("install-plan", "tools/call", {
      name: "menv_plan_environment_change",
      arguments: { action: "install", artifact_id: "php-8.3" },
    }));
    const install = JSON.parse(installResponse ?? "null");
    expect(install.result.structuredContent).toMatchObject({ ok: false, error: { code: "INVALID_PARAMS" } });
  });

  it("maps register_path to the core runtime.add action and rejects unknown nested fields", async () => {
    const service = testService();
    const server = createMcpServer({ service });
    const aliasResponse = await server.handleLine(request("register", "tools/call", {
      name: "menv_plan_environment_change",
      arguments: { change: { action: "register_path", path: "C:\\php\\php.exe" } },
    }));
    const alias = JSON.parse(aliasResponse ?? "null");
    expect(alias.result.structuredContent).toMatchObject({ ok: true, result: { summary: { action: "runtime.add" } } });

    const invalidResponse = await server.handleLine(request("nested-invalid", "tools/call", {
      name: "menv_plan_environment_change",
      arguments: { change: { action: "enable", runtime_id: "php-8.3", arbitrary: true } },
    }));
    const invalid = JSON.parse(invalidResponse ?? "null");
    expect(invalid.result.structuredContent).toMatchObject({ ok: false, error: { code: "INVALID_PARAMS" } });

    const mixedResponse = await server.handleLine(request("mixed", "tools/call", {
      name: "menv_plan_environment_change",
      arguments: { action: "enable", change: { action: "disable", runtime_id: "php-8.3" } },
    }));
    const mixed = JSON.parse(mixedResponse ?? "null");
    expect(mixed.result.structuredContent).toMatchObject({ ok: false, error: { code: "INVALID_PARAMS" } });

    const implicitRegisterResponse = await server.handleLine(request("implicit-register", "tools/call", {
      name: "menv_plan_environment_change",
      arguments: { action: "enable", path: "C:\\php\\php.exe" },
    }));
    const implicitRegister = JSON.parse(implicitRegisterResponse ?? "null");
    expect(implicitRegister.result.structuredContent).toMatchObject({ ok: false, error: { code: "INVALID_PARAMS" } });
  });

  it("does not emit a response for notifications", async () => {
    const server = createMcpServer({ service: testService() });
    await expect(server.handleLine(notification("notifications/initialized"))).resolves.toBeUndefined();
  });

  it("keeps stdout line-delimited JSON-RPC when serving stdio", async () => {
    const input = new PassThrough();
    const output = new PassThrough();
    const chunks: Buffer[] = [];
    output.on("data", (chunk: Buffer) => chunks.push(Buffer.from(chunk)));
    const serving = runStdioMcp({ service: testService(), input, output });
    input.end(`${request(1, "initialize")}\n${notification("notifications/initialized")}\n${request(2, "tools/list")}\n`);
    await serving;
    const lines = parseLines(Buffer.concat(chunks).toString("utf8"));
    expect(lines).toHaveLength(2);
    expect(lines.every((line) => (line as { jsonrpc?: string }).jsonrpc === "2.0")).toBe(true);
  });

  it("uses JSON-RPC parse and method errors without writing to core", async () => {
    const service = testService();
    const server = createMcpServer({ service });
    const parseError = JSON.parse((await server.handleLine("not-json")) ?? "null") as { error: { code: number } };
    expect(parseError.error.code).toBe(-32700);
    const methodError = JSON.parse((await server.handleLine(request(2, "bogus/method"))) ?? "null") as { error: { code: number } };
    expect(methodError.error.code).toBe(-32601);
    expect(service.calls).toHaveLength(0);
  });
});
