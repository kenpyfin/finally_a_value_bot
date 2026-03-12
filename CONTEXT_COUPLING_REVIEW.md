# Message-to-LLM Context Coupling Review

## Overview

This document analyzes how user messages flow through FinallyAValueBot and how they are coupled with contextual information before being sent to the LLM (Language Model). The flow is orchestrated across multiple channels (Telegram, Discord, WhatsApp) but uses a unified message processing pipeline.

---

## 1. Message Reception & Extraction

### Telegram (`src/channels/telegram.rs::handle_message`)
- **Entry point**: `handle_message()` receives teloxide `Message` objects
- **Content extraction** (lines 204-437):
  - Text from `msg.text()` or caption
  - Images: Downloaded, base64-encoded with media type detection (JPEG, PNG, GIF, WebP)
  - Voice: Transcribed via OpenAI Whisper if `openai_api_key` configured
  - Documents: Downloaded and saved locally with path metadata
  - **XML Sanitization** (lines 19-35): User input escaped before wrapping in `<user_message sender="...">` tags to prevent prompt injection

### Discord & WhatsApp
- Similar extraction patterns using their respective SDKs
- Same XML sanitization applied to user content

---

## 2. Preliminary Checks & Storage

### Permission & Group Checks (lines 467-514)
1. **Persona Resolution**: Fetches `persona_id` from database for the chat
2. **Group Allowlist**: If group chat and allowlist configured, validates membership
3. **Mention Check**: For group chats, requires bot mention (`@bot_username`)
4. **Message Storage**: All received messages stored in SQLite immediately:
   - `chat_id`, `persona_id`, `sender_name`, `content`, `is_from_bot`, `timestamp`
   - Stored even if group is not allowed (for audit/history)

### Async Execution
- Message processing spawned in background task (lines 579-640)
- Typing indicator refreshed every 4 seconds during processing
- Prevents webhook/request timeouts from interrupting agent execution

---

## 3. Core Agent Processing (`process_with_agent_with_events`)

### 3.1 System Prompt Construction (lines 695-778, 1381-1484)

The system prompt is built from **6 layers of context**:

#### Layer 1: Core Capabilities
```
- Bash execution
- Browser automation
- File operations (read/write/edit/glob/grep)
- Web search/fetch
- Memory management (tiered)
- Scheduling
- Message sending
- Skill activation
- etc.
```
Includes social media tools if configured (TikTok, Instagram, LinkedIn).

#### Layer 2: Principles (workspace_dir/AGENTS.md)
- **Highest priority** — loaded from `workspace_root/AGENTS.md`
- Survival mechanism: persists across session resets
- Global rules and agent identity
- Example: guidelines for tool usage, communication style

#### Layer 3: Memory Context (Per-Persona)
**Build via** `state.memory.build_memory_context(chat_id, persona_id)`:
- **Tier 1** (Long-term): Principles-like, persistent across sessions
- **Tier 2** (Active Projects): Current goals and ongoing work
- **Tier 3** (Recent Focus): Daily mood, recent context, not a todo list
- **Daily Log**: Today's and yesterday's append-only log

Location: `runtime/groups/{chat_id}/{persona_id}/MEMORY.md`

#### Layer 4: Workspace Context
- Loaded from files in `workspace_root/` (TOOLS.md, README.md, etc.)
- Document how the bot has been configured and what tools are available
- Provides continuity between sessions

#### Layer 5: Vault & Vector DB Paths (Optional)
- ORIGIN vault path
- ChromaDB vector database path
- Embedding server URL
- Custom search/index commands

#### Layer 6: Skills Catalog
- Discovered skills from `workspace_root/skills/` and `workspace_root/shared/skills/`
- Summary of each skill's purpose and when to use

### 3.2 Message History Assembly (lines 780-819)

**Two paths depending on session existence:**

#### Path A: Session Exists
1. Load saved session from database: `db.load_session(chat_id, persona_id)`
2. Deserialize JSON → `Vec<Message>`
3. Fetch new user messages since session updated: `db.get_new_user_messages_since()`
4. Append new messages to session, merging consecutive user messages
5. **Merging**: If last message is user role, append new user messages to same message block

#### Path B: No Session
1. Load from DB history using `load_messages_from_db()`
2. **For group chats**: Fetch all messages since last bot response (catch-up)
3. **For private chats**: Fetch last N messages (default: 50, configurable via `max_history_messages`)
4. Convert `StoredMessage` → `Message` via `history_to_claude_messages()` (lines 1486-1529)

### 3.3 User Message Formatting (lines 29-35, 1495-1496)

Each user message wrapped in XML:
```xml
<user_message sender="sanitized_name">sanitized_content</user_message>
```

**Sanitization escapes**: `&`, `<`, `>`, `"`

This prevents user input from breaking out of the prompt structure.

### 3.4 Image Handling (lines 829-850)

- If image data present, convert last user message to **blocks format**
- Blocks: `[Image { source, base64_data, media_type }, Text { ... }]`
- Images appear as content blocks to the LLM (Claude Vision support)

### 3.5 Message Compaction (lines 857-877)

**Trigger**: When `messages.len() > max_session_messages` (default: 50)

**Process**:
1. **Memory Flush**: Run silent agent loop to let model write important facts to memory before compacting
2. **Archive**: Save full transcript to markdown file
3. **Summarize**: Call `compact_messages()` to reduce token count
   - Typically summarizes old messages into a single summary block
   - Keeps recent messages intact

---

## 4. Optional: Orchestrator Plan (Plan-First Architecture)

### When Enabled: `orchestrator_enabled=true` (lines 887-1022)

1. **Plan Step** (lines 925-932):
   - Timeout: 30 seconds
   - Analyzes last user message + recent context (last 4 messages)
   - Returns JSON: `{ "strategy": "direct|delegate", "delegate_tasks": [...] }`

2. **If Strategy = "delegate"** (lines 935-1005):
   - Spawn sub-agents for each task (via `sub_agent` tool)
   - Compile results with 120-second timeout
   - Append compiled results to message history as orchestrator context
   - Fall through to main agent loop

3. **If Strategy = "direct"** or timeout:
   - Skip delegation, proceed to main agent loop

**Note**: Orchestrator uses cheaper/faster model if configured (`orchestrator_model`)

---

## 5. Agentic Tool-Use Loop (lines 1024-1334)

### 5.1 LLM Request Structure (lines 1036-1104)

```rust
state.llm.send_message(
    system_prompt: &str,                    // Full system context
    messages: Vec<Message>,                 // Conversation history
    tools: Option<Vec<ToolDefinition>>,    // Available tools + definitions
)
```

### 5.2 Request Building (claude.rs, MessagesRequest)

```json
{
  "model": "claude-3-5-sonnet-20241022",
  "max_tokens": 8192,
  "system": "<full system prompt>",
  "messages": [
    { "role": "user", "content": "<user_message sender=\"...\">...</user_message>" },
    { "role": "assistant", "content": "Response text" },
    { "role": "user", "content": { "blocks": [ { "type": "tool_result", ... } ] } }
  ],
  "tools": [
    {
      "name": "bash",
      "description": "Execute bash commands",
      "input_schema": { "properties": { "command": { "type": "string" } }, ... }
    },
    ...
  ]
}
```

### 5.3 Loop Control (lines 1030-1282)

**For each iteration (max: `max_tool_iterations`, default 100):**

1. **Call LLM** (180s timeout):
   - If streaming: collect text deltas and send to event channel
   - Parse response: extract `stop_reason` and `content` blocks

2. **Stop Reason Handling**:
   - **"end_turn"**: Extract text, save session, return response
   - **"tool_use"**: Extract tool calls, execute, append results, loop
   - **"max_tokens"**: Return response with truncation notice

3. **Tool Execution** (120s timeout per tool):
   - Build `ToolAuthContext`: validates permission (chat_id, persona_id, control_chat_ids)
   - Execute tool via registry: `tools.execute_with_auth(name, input, auth_context)`
   - Collect results as `ContentBlock::ToolResult { tool_use_id, content, is_error }`
   - Append to messages: `Message { role: "user", content: Blocks(tool_results) }`

4. **Session Saving**:
   - After each iteration: save messages JSON to database
   - Prevents loss of work on crash
   - Session is the conversation state: full `Vec<Message>`

---

## 6. Response Finalization

### 6.1 Post-Processing (lines 1109-1148)

1. **Strip Thinking Blocks**: If `show_thinking=false` (default), remove `<think>...</think>` blocks
2. **Extract Text**: Collect all text content blocks
3. **Ensure Non-Empty**: Replace empty responses with "Done."
4. **Save Final Session**: Store completed conversation state

### 6.2 Sending Response (lines 605-629)

- **Telegram**: Split at 4096 chars (Telegram limit)
- **Discord**: Split at 2000 chars
- Parse to appropriate format (Markdown → HTML for Telegram)
- Store bot response in database as `StoredMessage`

---

## 7. Data Structures

### Message (claude.rs)
```rust
pub struct Message {
    pub role: String,                    // "user" | "assistant"
    pub content: MessageContent,
}

pub enum MessageContent {
    Text(String),                        // Plain text
    Blocks(Vec<ContentBlock>),          // Mixed content (text, images, tool_use/results)
}

pub enum ContentBlock {
    Text { text: String },
    Image { source: ImageSource },      // base64-encoded
    ToolUse { id, name, input },        // LLM requesting tool execution
    ToolResult { tool_use_id, content, is_error },  // Tool result for LLM
}
```

### Database (StoredMessage)
```rust
pub struct StoredMessage {
    pub id: String,                      // UUID or Telegram message ID
    pub chat_id: i64,
    pub persona_id: i64,
    pub sender_name: String,
    pub content: String,                 // Original text (not XML-wrapped)
    pub is_from_bot: bool,
    pub timestamp: String,               // RFC3339
}
```

---

## 8. Context Hierarchy

When conflicts arise, priority is **top-to-bottom**:

1. **Principles** (AGENTS.md) — highest priority, global rules
2. **System Prompt Instructions** — tool capabilities, chat rules
3. **Persona Memory** (Tier 1 → 2 → 3) — recent context
4. **Workspace Documentation** — project setup
5. **Conversation History** — recent messages
6. **Orchestrator Results** — if enabled

---

## 9. Security & Isolation

### Permission Model
- **Non-control chats**: Can only operate on their own `chat_id`
- **Control chats**: Can perform cross-chat actions (send_message to other chats, etc.)
- **Enforcement**: ToolAuthContext checks on every tool execution

### Path Guard
- Sensitive paths blocked: `.ssh`, `.env`, `.aws`, `/etc/shadow`, etc.
- Prevents tools from accessing host system secrets

### XML Escaping
- User input can't break out of `<user_message>` wrapper
- Prevents prompt injection via chat messages

---

## 10. Message Flow Diagram

```
User Message (Telegram/Discord/WhatsApp)
    ↓
[Extract content: text, images, voice, documents]
    ↓
[Check persona, permissions, allowlist]
    ↓
[Store in SQLite]
    ↓
[Load or Build System Prompt]
    ├─ Principles (AGENTS.md)
    ├─ Capabilities list
    ├─ Memory context (per-persona)
    ├─ Workspace docs
    └─ Skills catalog
    ↓
[Load or Assemble Messages]
    ├─ Load session OR
    └─ Fetch from DB + format
    ↓
[Optional: Run Orchestrator (plan)]
    ├─ If delegate: spawn sub-agents
    └─ Append sub-agent results
    ↓
[Agentic Loop: while iteration < max]
    ├─ Send: system + messages + tools → LLM
    ├─ Parse: stop_reason + response
    ├─ If tool_use:
    │   ├─ Execute tool (with permission check)
    │   ├─ Append result as user message
    │   └─ Loop
    ├─ Else if end_turn:
    │   ├─ Extract text
    │   ├─ Save session
    │   └─ Return
    └─ [Handle timeouts and errors]
    ↓
[Post-processing]
    ├─ Strip thinking blocks (unless enabled)
    └─ Ensure non-empty response
    ↓
[Send Response to User + Store in DB]
```

---

## 11. Key Files & Line References

| Component | File | Lines |
|-----------|------|-------|
| Telegram message handler | `src/channels/telegram.rs` | 197-643 |
| Core agent processing | `src/channels/telegram.rs` | 685-1335 |
| System prompt building | `src/channels/telegram.rs` | 1381-1484 |
| Message loading | `src/channels/telegram.rs` | 1338-1377 |
| XML escaping | `src/channels/telegram.rs` | 19-35 |
| LLM interface | `src/llm.rs` | 150-176 |
| Message types | `src/claude.rs` | 1-98 |
| Database storage | `src/db.rs` | (not shown, but handles persistence) |
| Tool execution | `src/tools/mod.rs` | (not shown, but handles auth & execution) |

---

## 12. Observations & Recommendations

### Current Design Strengths
1. **Clear separation**: Message reception → context building → LLM request → tool execution
2. **Robust context**: Multi-layer system prompt ensures continuity across sessions
3. **Security**: XML escaping, permission model, path guards prevent attacks
4. **Session persistence**: Conversation state saved after each iteration
5. **Timeout handling**: Multiple timeouts (LLM, tools) prevent infinite hangs

### Potential Improvements
1. **Context size monitoring**: Track token counts to predict compaction trigger
2. **Memory tier automation**: Could suggest tier updates based on conversation patterns
3. **Tool metadata**: Document expected inputs/outputs per tool more explicitly
4. **Error recovery**: Some tool errors could be retried with backoff
5. **Conversation branching**: Support multiple parallel conversation paths (not just session save/load)

---

## Summary

User messages flow through a sophisticated context-coupling system:
- **Channel-specific extraction** → **XML-wrapped, sanitized text**
- **Permission & persona checks** → **Permission model enforcement**
- **Multi-layer system prompt** (principles, memory, workspace, skills)
- **Session or DB-sourced history** → **Conversation state**
- **Optional orchestrator planning** → **Task delegation or direct response**
- **Agentic loop** (LLM call → tool execution → result appending)
- **Session saving** after each iteration + final response delivery

This design ensures:
- **Consistency** across sessions via persistent memory
- **Security** via permission checks and path guards
- **Reliability** via timeouts and error handling
- **Extensibility** via skills and tool registry
