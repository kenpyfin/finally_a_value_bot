---
name: cursor-agent
description: Guide for using the Cursor Agent CLI for advanced development, refactoring, and codebase-wide changes.
license: MIT
compatibility:
  os:
    - darwin
    - linux
  deps:
    - cursor-agent
---

# Cursor Agent

Use this skill when you need to perform complex, multi-file development tasks that exceed the typical capabilities of single-file edits. This skill guides the agent on how to use the `cursor-agent` CLI tool efficiently.

## Capabilities

- **Codebase-wide Refactoring**: Apply changes across many files at once.
- **New Feature Implementation**: Scaffold and implement entire modules.
- **Bug Discovery and Fixing**: Search for patterns and fix them globally.
- **Documentation Updates**: Synchronize docs with code changes.

## When to use

- When the task is too large for the main agent's context.
- When you need a specialized sub-agent that is optimized for codebase research and modification.
- When the user explicitly asks to "use cursor-agent" for a task.

## How to use

Invoke the `cursor_agent` tool or call the CLI directly via `bash`.

### Basic invocation (via bash)

```bash
cursor-agent "Implement a new authentication layer in src/auth"
```

### Detached mode (for long-running tasks)

If tmux is available and supported, use `detach: true` in the `cursor_agent` tool to run it in the background.

```bash
# Example logic for the agent
# 1. Decide on a clear prompt.
# 2. Call cursor_agent tool with prompt and detach: true.
# 3. Monitor the run via list_cursor_agent_runs.
```

## Tips for Better Results

1. **Clear Prompts**: Provide a detailed description of what needs to be changed.
2. **Context**: Mention specific files or directories that are relevant.
3. **Verification**: Always verify the changes made by the agent once it's finished.
4. **Iterate**: If the first run doesn't get it quite right, provide a follow-up prompt to the agent.

## Limitations

- The agent is an external process and does not share your immediate conversational memory unless explicitly provided in the prompt.
- Do not use for tasks requiring interactive user input.
