import { MCP_PROTOCOL_VERSION, MCP_TOOL_DEFINITIONS } from "../src/index.js";

if (MCP_PROTOCOL_VERSION !== "2025-11-25") {
  throw new Error(`Unexpected MCP protocol version: ${MCP_PROTOCOL_VERSION}`);
}

const names = new Set<string>();
for (const tool of MCP_TOOL_DEFINITIONS) {
  if (names.has(tool.name)) throw new Error(`Duplicate MCP tool: ${tool.name}`);
  names.add(tool.name);
  if (tool.inputSchema.additionalProperties !== false) {
    throw new Error(`MCP tool ${tool.name} must reject unknown fields`);
  }
}

process.stdout.write(`validated ${MCP_TOOL_DEFINITIONS.length} MCP tools\n`);
