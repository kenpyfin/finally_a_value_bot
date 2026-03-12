---
name: social-feed
description: Fetch TikTok, Instagram, or LinkedIn feeds for the user. Use when the user asks to see their feed, recent posts, or videos from these platforms. Requires one-time OAuth per user per platform.
---

# Social Feed Tools

Use this skill when the user asks to:
- Fetch their TikTok videos
- Get their Instagram posts
- See their LinkedIn feed
- "Show my recent posts from [platform]"

## Available tools

The following tools are available (only if configured in finally-a-value-bot.config.yaml under `social`):

| Tool | Platform | Use when |
|------|----------|----------|
| `fetch_tiktok_feed` | TikTok | User asks for their TikTok videos |
| `fetch_instagram_feed` | Instagram | User asks for their Instagram posts |
| `fetch_linkedin_feed` | LinkedIn | User asks for their LinkedIn posts |

## How to use

1. **Call the appropriate tool** — Use `fetch_tiktok_feed`, `fetch_instagram_feed`, or `fetch_linkedin_feed` based on which platform the user requested.
2. **Omit username** — To fetch the user's own feed, call the tool without a `username` parameter.
3. **Optional parameters** — You can pass `max_items` (default 10) and `cursor` (for pagination) if needed.

## First-time authorization (OAuth)

If the user has not authorized the platform before, the tool will return an authorize link. **You must show this link to the user** so they can click it, log in on the platform, and grant access.

Example response when not authorized:
> "To fetch your LinkedIn feed, you must authorize first. Click this link to connect: [URL]"

**Tell the user to click the link** and complete the authorization. Once done, they can ask again and the tool will return their feed.

## Limitations

- **Own feed only** — These tools fetch the *user's own* feed. Public profile by username is not supported by the platform APIs.
- **Per-platform** — Each platform (TikTok, Instagram, LinkedIn) requires separate authorization.
- **Configuration** — Tools are only available if the admin has added `social` config with `client_id` and `client_secret` for each platform.

## Example flow

1. User: "Get my LinkedIn feed"
2. You: Call `fetch_linkedin_feed` with no username
3. If authorized: Tool returns posts; summarize and show to user
4. If not authorized: Tool returns authorize link; tell user to click it, then ask them to try again after they've completed authorization
