---
layout: page
title: MCP (Model Context Protocol) server
permalink: /mcp/
---

## Overview

OpenTune exposes a **Model Context Protocol (MCP)** server running on your local machine at `http://127.0.0.1:8765/mcp` (default port). This allows external AI agents — running inside Claude Code, Claude Desktop, or other MCP-compatible clients — to analyze your tune and propose changes using the same deterministic tools as the embedded assistant.

The server is **advisory-only**: agents can read tune data and propose changes, but cannot write directly to the ECU. Every proposal must be reviewed and applied manually inside the OpenTune application.

## Enabling the server

1. Launch OpenTune.
2. Navigate to **Settings** → **AI**.
3. Toggle **Expose tools over MCP (local)** to on.
4. The server starts immediately and listens on `127.0.0.1:8765` (or your configured port).
5. To stop, toggle it off or close OpenTune.

The server only runs while OpenTune is active with the toggle enabled.

## Authentication

Every MCP request must include a bearer token in the `Authorization` header. The token is unique per installation and is shown in the **AI settings panel**.

**To regenerate the token** (instantly invalidating the old one):
1. Open **Settings** → **AI**.
2. Click **Regenerate**.
3. Update your client configuration with the new token.

Tokens are stored in the app config directory (`mcp-token` file) and are not logged.

## Security

- **Loopback-only binding:** the server binds to `127.0.0.1` exclusively, preventing network access.
- **Constant-time token comparison:** bearer token is checked using constant-time comparison to prevent timing attacks.
- **Origin validation:** requests are validated against loopback origins.
- **Audit logging:** every MCP tool call is appended to `ai-audit.jsonl` in the app config directory with channel `mcp`.

## Available tools

The server exposes seven tools from the deterministic analysis engine (see [Architecture § 5.9]({{ '/architecture/' | relative_url }})):

### Read-only tools

- **`read_tune`** — return the current tune as a map of field name → value.
- **`read_realtime`** — snapshot of live sensor data (engine speed, coolant temp, oxygen, etc.).
- **`run_ve_analyze`** — suggest VE table changes based on logged datalog.
- **`get_log_stats`** — summary statistics from a datalog (min/max/mean for all channels).
- **`detect_anomaly`** — identify unusual patterns in logged data (sensor glitches, fuel enrichment anomalies).
- **`virtual_dyno`** — estimate power and torque from logged data.

### Mutating tool

- **`propose_change`** — suggest a change to one or more tune fields.
  - Validates against guardrails: maximum 15% delta per cell, maximum 64 cells per proposal, minimum 1000 ms between proposals.
  - Returns a proposal ID and detailed justification.
  - **Does not write to the ECU** — changes surface in the **Proposals** panel in OpenTune for manual review and apply.

No `apply_change` or `burn_now` tools are exposed — the user retains full control over when changes are written to hardware.

## Connecting Claude Code

```bash
claude mcp add --transport http opentune http://127.0.0.1:8765/mcp \
  --header "Authorization: Bearer TOKEN"
```

Replace `TOKEN` with the **Access token** shown under the **MCP server** section in **Settings** → **AI**.

## Connecting Claude Desktop

Claude Desktop does not support static headers in its UI config, so use the `npx mcp-remote` Node.js bridge:

```json
{
  "mcpServers": {
    "opentune": {
      "command": "npx",
      "args": ["mcp-remote", "http://127.0.0.1:8765/mcp", "--allow-http",
               "--transport", "http-only",
               "--header", "Authorization:${AUTH_HEADER}"],
      "env": { "AUTH_HEADER": "Bearer TOKEN" }
    }
  }
}
```

Replace `TOKEN` with your token. **Note:** Node.js must be installed on your machine.

The space in `Authorization:${AUTH_HEADER}` is deliberately placed in the environment variable to work around a platform-specific escaping issue in Claude Desktop.

## Configuration

The MCP server port can be changed in **Settings** → **AI** under the **MCP server** heading via the **Port** field (minimum 1024). The server automatically restarts on port changes or when the toggle is toggled.

## Workflow example

1. **Start OpenTune** with a tune loaded and MCP enabled.
2. **Connect Claude Code** using the `claude mcp add` command above.
3. **Ask Claude Code** to analyze your tune:
   - `Read my tune and suggest a VE table change based on what you find.`
   - `Analyze this datalog and detect anomalies.`
   - `Estimate the power output from my last recorded run.`
4. Claude will call `read_tune`, `read_realtime`, or `run_ve_analyze`.
5. If a proposal is made, it appears in **OpenTune** → **Proposals** panel.
6. Review the proposed change and click **Apply** to write it to the ECU.

## Rate limiting

Proposals are rate-limited to one per 1000 milliseconds (1 proposal per second). Exceeding this rate returns an error.

## Debugging

- Check `ai-audit.jsonl` in the app config directory for a log of all MCP calls and responses.
- If authentication fails, verify the token wasn't regenerated from the settings panel since the client was configured — open **Settings** → **AI** and copy the current token again.
- If the server fails to start, check that port 8765 (or your configured port) is not in use; use `lsof -i :8765` (macOS/Linux) or `netstat -ano` (Windows) to diagnose.

## Related

- [Architecture § 5.9]({{ '/architecture/' | relative_url }}) — Deterministic analysis tools
- [Architecture § 5.10]({{ '/architecture/' | relative_url }}#510-ai--the-ai-orchestration-layer-built-on-analysis) — AI layer design and embedded assistant guide (how the same tools power the in-app AI chat)
