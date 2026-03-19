---
name: searxng-search
description: Use SearXNG-powered web search at full potential—science, files/PDFs, images, news, time range, and query syntax. Activate when SEARXNG_URL is set and the user needs advanced or targeted search.
license: MIT
compatibility:
  os:
    - linux
    - darwin
    - windows
deps: []
---

# SearXNG Search

Use this skill when **SEARXNG_URL** is set in the environment and the user needs web search. The **web_search** tool then uses your SearXNG instance and supports categories, engines, time range, and rich query syntax. Use it for general web search, **science/academic**, **files and PDFs**, images, news, and time-limited search.

## Prerequisite

**SEARXNG_URL** must be set in `.env` (e.g. to a public instance or your self-hosted SearXNG). If it is not set, web_search falls back to DuckDuckGo and does not support the options below.

## Tool: web_search

When SearXNG is used, **web_search** accepts:

| Parameter     | Required | Description |
|---------------|----------|-------------|
| `query`       | Yes      | Search query. Can include SearXNG syntax (see below). |
| `categories`  | No       | Comma-separated categories to search. Use to restrict to a tab (e.g. science, files). |
| `engines`     | No       | Comma-separated engine names to use (e.g. google, semantic_scholar). |
| `time_range`  | No       | Limit results to: `day`, `week`, `month`, `year`. |

## Categories (use with `categories` or in-query `!`)

Use the **categories** parameter, or the **!category** syntax inside the query. Common categories:

| Category  | Use for |
|-----------|--------|
| `general` | Default web search (multiple engines). |
| `science` | Academic and scientific results (Semantic Scholar, Crossref, PubMed, etc.). |
| `files`   | File and document search (LibGen, Wikimedia Commons, etc.); good for PDFs and books. |
| `images`  | Image search. |
| `it`      | IT/tech (Stack Overflow, Arch Wiki, etc.). |
| `news`    | News articles. |
| `map`     | Maps and places. |
| `videos`  | Video search. |
| `music`   | Music. |
| `social`  | Social platforms. |

Examples:

- **Science search:**  
  `web_search(query="CRISPR gene editing", categories="science")`  
  or in query: `!science CRISPR gene editing`

- **File/PDF search:**  
  `web_search(query="filetype:pdf reinforcement learning", categories="files")`  
  or: `!files deep learning survey filetype:pdf`

- **Recent only:**  
  `web_search(query="climate report", time_range="month")`

## Query syntax (inside `query`)

You can mix these in the **query** string; SearXNG and underlying engines interpret them where supported.

- **Category/engine in query:**  
  `!science`, `!files`, `!images`, `!news`, `!map`, `!wp` (Wikipedia), `!go` (Google), `!ddg` (DuckDuckGo).  
  Example: `!science !wp quantum computing`

- **Language:**  
  `:en`, `:fr`, etc.  
  Example: `:en machine learning`

- **File type (when engines support it):**  
  `filetype:pdf`, `filetype:doc`, `filetype:xlsx`.  
  Example: `filetype:pdf survey neural networks`

- **Site restrict:**  
  `site:arxiv.org`, `site:github.com`.  
  Example: `site:semanticscholar.org transformers`

- **Time intent:**  
  Prefer the **time_range** parameter for reliable filtering. You can also try phrases like “2024” or “recent” in the query.

## Science and academic search

- Set **categories** to `science` and use a clear topic in **query**.  
  Example: `web_search(query="large language model benchmarks", categories="science")`.

- To favor specific engines:  
  `web_search(query="attention mechanism", categories="science", engines="semantic_scholar,crossref")`  
  (Exact engine names depend on the instance; common: `semantic_scholar`, `crossref`, `pubmed`, `arxiv`.)

- Combine with **site:** for a given repository:  
  `query="site:arxiv.org variational autoencoder"`.

## File and PDF search

- Use category **files** and, when you want PDFs only, add **filetype:pdf** in the query.  
  Example: `web_search(query="filetype:pdf optimization algorithms", categories="files")`.

- Some instances expose **libgen** or similar in the files category; if the user asks for books or papers, use `categories="files"` and a clear query (title, author, or topic).

## Images, news, and others

- **Images:**  
  `web_search(query="diagram of cell mitosis", categories="images")` or `!images ...`.

- **News:**  
  `web_search(query="election results", categories="news", time_range="week")`.

- **Maps:**  
  `web_search(query="coffee shops downtown", categories="map")` or `!map ...`.

## Best practices

1. **Prefer the right category** (science, files, images, news, etc.) instead of generic “general” when the user’s intent is clear.
2. **Use time_range** for “recent”, “this week”, “this year” instead of only adding words to the query.
3. **Use filetype:pdf** (and category **files** when appropriate) for PDF-only or document-focused search.
4. **Combine parameters:** e.g. `categories="science"`, `time_range="year"`, and `query="site:arxiv.org ..."` for recent arXiv papers.
5. Instance capabilities vary: if a category or engine returns no results, try `general` or a simpler query.

## Examples (concise)

- General: `web_search(query="how to use rust async")`
- Science: `web_search(query="transformer architecture", categories="science")`
- PDFs: `web_search(query="filetype:pdf BERT survey", categories="files")`
- Recent news: `web_search(query="AI regulation", categories="news", time_range="month")`
- In-query syntax: `web_search(query="!science !wp CRISPR")` (Wikipedia + science category)
