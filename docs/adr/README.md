# Architecture Decision Records

These records capture durable QuickLook Next boundaries. They explain why a
guarded invariant exists; detailed protocol fields and current capability tables
remain in their focused technical documents and source contracts.

- [ADR-0001: Supervise preview hosts](0001-supervised-preview-hosts.md)
- [ADR-0002: Transfer HANDLE ownership, not path authority](0002-handle-ownership-and-path-authority.md)
- [ADR-0003: Cancel by generation and reject stale work](0003-cancellation-and-stale-work.md)
- [ADR-0004: Use typed, request-bound preview errors](0004-preview-error-contracts.md)

When a decision changes, add a superseding ADR and update guards and tests in
the same change. Do not silently rewrite an accepted boundary to describe a new
architecture.
