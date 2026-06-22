#!/usr/bin/env bash
# Scripted fake-claude for VHS demo recording.
# Run via: SPYC_PANE_CMD=./docs/demo/fake-claude.sh spyc
# Prints a realistic Claude Code interaction, fully deterministic.

# Claude startup banner
printf '\n'
printf '\033[2m/Users/derek/src/spyc\033[0m\n'
printf '\033[1;35m>\033[0m \033[1mClaude\033[0m \033[2m(claude-opus-4-5 · claude-code)\033[0m\n'
printf '\033[32m  ✓ spyc MCP connected\033[0m\n'
printf '\n'
printf '\033[2mType /help for help, /status for account · \033[0m'
printf '\033[32m● MCP: spyc\033[0m\n'
printf '\n'

# First prompt
printf '\033[1;35m>\033[0m '
read -r _question
printf '\n'

# Thinking indicator
printf '\033[33m⠦\033[0m Thinking...\n'
sleep 0.6

# MCP call
printf '\n'
printf '\033[2m  ⎿ Tool: get_spyc_context\033[0m\n'
sleep 0.5
printf '\033[2m  ⎿ {"cwd":"/Users/derek/src/spyc","cursor_file":"src/app/mod.rs","git_branch":"main"}\033[0m\n'
sleep 0.4
printf '\n'

# Response body
cat <<'RESPONSE'
The MCP tool dispatch flows through two files:

  src/app/loop_steps.rs:93   — drain_mcp_pending(), drains queued tool calls
  src/app/mcp.rs:232         — execute_mcp_command(), the actual handler

When an agent calls a tool over the Unix socket, spyc's MCP listener
thread pushes an Mcp(McpRequest) message into the main loop. On each
loop iteration, drain_mcp_pending() pops the queue and calls
execute_mcp_command() per request, which pattern-matches on the command
name ("get_spyc_context", "search_paths", etc.) and returns the result.

To add a new MCP tool, add the match arm in src/app/mcp.rs:232.

RESPONSE

printf '\033[2m  Press gf in the file list to jump to any path above.\033[0m\n'
printf '\n'
printf '\033[1;35m>\033[0m '

# Stay alive so spyc doesn't restart the pane
while true; do read -r _; done
