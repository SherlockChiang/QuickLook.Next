# Security Policy

QuickLook Next previews untrusted files and uses native parsers, Windows codecs, and isolated helper
processes. Please report security problems privately so maintainers can investigate before details or
malicious samples become public.

## Supported Versions

Security fixes target:

- The latest stable release published in this repository.
- The current `main` branch when the issue affects unreleased code.

Older releases may receive guidance, but users should expect to update to the newest fixed release.

## Reporting A Vulnerability

1. Use GitHub's **Report a vulnerability** action in this repository's Security tab when it is
   available.
2. If private vulnerability reporting is unavailable, contact the repository owner through their
   GitHub profile with only a minimal description and ask for a private reporting channel.
3. Do not open a public issue, discussion, or pull request during the initial coordination period.

Include, when available:

- Affected QuickLook Next version or commit.
- Windows version and architecture.
- The affected preview format and route (`App`, `ParserHost`, `RasterHost`, or `ShellBroker`).
- Reproduction steps, expected behavior, and observed security impact.
- Whether the issue crosses a process, HANDLE, pipe, filesystem, cloud-download, or signing boundary.
- A minimized proof of concept that contains no personal or third-party confidential data.

Do not send passwords, signing keys, access tokens, full private documents, crash dumps containing
private paths, or unredacted logs through a public channel. Keep a malicious sample encrypted until a
maintainer provides an approved transfer method; send the decryption secret separately.

## Useful Security Reports

Examples include:

- Code execution, sandbox or restricted-token escape, or mitigation bypass.
- Unauthorized file reads/writes, path traversal, unsafe archive extraction, or HANDLE confusion.
- Named-pipe authentication, process-identity, protocol-validation, or stale-request failures.
- Unbounded parsing, decompression, allocation, GPU-surface, or cloud-hydration behavior that can
  materially exhaust system resources.
- Update, package, checksum, SBOM, certificate, or release-signing compromise.
- A privacy boundary that reads or transmits file content without the documented user action.

Ordinary crashes, unsupported formats, and preview fidelity problems can use the public issue tracker
when they do not reveal an undisclosed security weakness or require a sensitive sample.

## Disclosure And Response

The project does not promise a fixed response SLA. Maintainers will try to acknowledge a complete
report, reproduce it, coordinate a fix and release, and agree on disclosure timing. Unless both sides
agree otherwise, reporters may disclose 90 days after the first good-faith private contact, or sooner
if the vulnerability is already being actively exploited and prompt disclosure is needed to protect
users. Before disclosure, make a reasonable attempt to notify the maintainer of the planned date.
Please avoid testing against systems or files you do not own or have permission to use.
