# Security policy

## Supported versions

We address security issues in the latest tagged release and on the default branch (`main` or `master`), whichever is current for this repository.

## Reporting a vulnerability

**Please do not** open a public issue for undisclosed security vulnerabilities.

1. Use **GitHub private security advisories** for this repository if the feature is enabled: [Security advisories](https://github.com/diffloom/diffloom/security/advisories).
2. Otherwise, contact maintainers privately (for example via GitHub profile email or org contact), with subject line including `SECURITY` and the project name.

Include where possible:

- A short description of the issue and its impact
- Steps to reproduce, or proof-of-concept, with any payloads redacted where appropriate
- Affected versions or commit SHA
- Suggested fix (optional)

We aim to acknowledge reports within a few business days and coordinate disclosure after a fix is available.

## Scope in scope

- The `diffloom` binary and library shipped from this repository
- MCP stdio handling and workspace path handling
- Network calls (for example optional LLM review via `DIFFLOOM_LLM_URL`) when they process untrusted input

Out of scope: issues in upstream dependencies unless they require a change in how Diffloom uses them; social engineering of users.

## Safe harbor

If you follow this policy and act in good faith, we will not pursue legal action against you for accidental, non-destructive research.
