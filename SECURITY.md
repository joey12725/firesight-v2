# Security

## Reporting a vulnerability

Please report security issues privately through [GitHub's private vulnerability reporting](https://github.com/joey12725/firesight-v2/security/advisories/new), not in a public issue. You should get a reply within a week.

## What is in scope

FIRESight has a small attack surface by design. There is no account system and no database, and simulation runs entirely in the browser. The parts that handle anything sensitive are:

- **Plans.** Stored in the browser (IndexedDB and localStorage) and never uploaded. Share links carry the plan in the URL fragment (after `#`), which browsers do not send to servers.
- **A user's own Anthropic API key.** Kept in the browser's localStorage and sent only to `api.anthropic.com`.
- **The hosted assistant proxy** (`api/chat.ts`). It adds the operator's key to requests that look like FIRESight's own, with an optional access code and per-IP rate limits. These checks deter casual misuse of the operator's key; they cannot stop a determined client, which is why operators should put a spend limit on the key.

[docs/assistant.md](docs/assistant.md#privacy-what-leaves-the-browser) describes exactly what is sent to Claude when the assistant is used.

Reports about any of the above, about cross-site scripting through imported plans or share links, or about the proxy forwarding requests it should refuse, are all welcome.
