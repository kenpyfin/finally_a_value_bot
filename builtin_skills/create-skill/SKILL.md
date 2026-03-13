---
name: create-skill
description: Guide for creating well-structured FinallyAValueBot skills that the agent can discover, activate, and use. Now with guidance on API rate limits and efficiency.
platforms:
  - linux
  - darwin
deps: []
---

# create-skill

---
name: create-skill
description: Guide for creating well-structured FinallyAValueBot skills that the agent can discover, activate, and use.
license: MIT
compatibility:
  os:
    - darwin
    - linux
    - windows
---

# Create Skill

Use this skill when the user asks you to create a new skill, build a skill, add a capability, or when you need to create a skill yourself. Follow this guide to produce a skill that integrates correctly with the FinallyAValueBot skill system.

## Skill structure

Every skill lives in its own directory under the skills folder:

```
skills/
  my-skill/
    SKILL.md          # Required — frontmatter + instructions
    .env              # Optional — credentials or config
    helper_script.py  # Optional — any supporting files
```

The only required file is `SKILL.md`. The skill manager discovers skills by scanning for `SKILL.md` (or `skill.md`) in subdirectories.

## SKILL.md format

The file must start with YAML frontmatter between `---` fences, followed by a markdown body:

```markdown
---
name: my-skill
description: One-line description of what this skill does and when to use it.
license: MIT
compatibility:
  os:
    - darwin
    - linux
  deps:
    - curl
    - python3
---

# My Skill

Instructions for the agent on how to use this skill.
```

### Required frontmatter fields

| Field | Type | Description |
|---|---|---|
| `name` | string | Skill name (should match the directory name). Lowercase, hyphens for spaces. |
| `description` | string | What the skill does. This is shown in skill listings and used for matching when the agent decides which skill to activate. Make it specific and action-oriented. |

### Optional frontmatter fields

| Field | Type | Description |
|---|---|---|
| `license` | string | License identifier (e.g. `MIT`, `Apache-2.0`). |
| `compatibility.os` | list | Target platforms: `darwin`, `linux`, `windows`. Omit to mean all. |
| `compatibility.deps` | list | Required external tools (e.g. `curl`, `python3`, `ffmpeg`). |
| `source` | string | URL for the upstream source, if adapted from elsewhere. |

### Body content

The markdown body after the frontmatter is what the agent reads when the skill is activated. Write it as instructions **for the agent**, not for a human. Include:

1. **When to use** — What kind of user requests trigger this skill.
2. **How to use** — Step-by-step instructions with concrete commands, code snippets, or tool invocations.
3. **Environment / prerequisites** — What needs to be installed or configured.
4. **Examples** — Realistic usage examples.
5. **Troubleshooting** — Common failure modes and fixes.

## Best practices

- **Write for the agent.** The SKILL.md is read by the AI agent, not a human developer. Be direct: "Run this command", "Use this tool", "Return this format".
- **Be specific.** Include exact commands, file paths, API patterns. Avoid vague instructions like "configure as needed".
- **Include the description trigger.** The `description` field determines when the skill gets activated. Use action words that match user intent: "Use this skill when the user wants to..." or "Use when the user asks to...".
- **Bundle scripts.** If the skill needs helper scripts (Python, bash, etc.), put them in the skill directory. Reference them with relative paths: `skills/my-skill/helper.py`.
- **Credentials in .env.** If the skill needs API keys or secrets, instruct the user to create `skills/my-skill/.env` with the required variables. Never hardcode secrets.
- **API Caution.** For any skill that makes API calls, you MUST be cautious about rate limits and usage. Optimize the call efficiency (e.g. batching, caching, or avoiding redundant calls) and include error handling for rate limit hits (e.g. 429 status codes).
- **Keep it focused.** One skill = one capability. Don't create a mega-skill that does everything.
- **Test the commands.** Before finalizing, verify that all commands and code snippets in the skill actually work.

## How to create a skill

Use the `build_skill` tool, which handles creating the directory and writing `SKILL.md`:

```
build_skill(name="my-skill", description="...", instructions="...")
```

Or create the files directly with `write_file`:

```
write_file(path="skills/my-skill/SKILL.md", content="---\nname: my-skill\n...")
```

After creating the skill, it becomes available immediately for activation via `activate_skill`.

## Example: minimal skill

```markdown
---
name: translate
description: Translate text between languages using the `trans` CLI tool. Use when the user asks to translate text.
license: MIT
compatibility:
  deps:
    - trans
---

# Translate

Use this skill when the user asks to translate text between languages.

## Usage

```bash
trans -b "Hello world" :es
```

Flags:
- `-b` — brief mode (translation only)
- `:es` — target language code (ISO 639-1)
- `-s en` — explicitly set source language

## Common language codes

| Language | Code |
|---|---|
| Spanish | es |
| French | fr |
| German | de |
| Japanese | ja |
| Chinese | zh |
```

## Example: skill with helper script

```markdown
---
name: image-resize
description: Resize and optimize images. Use when the user wants to resize, compress, or convert image files.
compatibility:
  deps:
    - python3
---

# Image Resize

Run the bundled Python script:

```bash
python3 skills/image-resize/resize.py input.png --width 800 --output resized.png
```

Supported formats: PNG, JPEG, WebP, GIF.
```

Then also create `skills/image-resize/resize.py` with the actual implementation.

---

Put credentials in `/home/ken/big_storage/projects/finally-a-value-bot/./workspace/skills/create-skill/.env` if needed.
