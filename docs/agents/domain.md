# Domain Docs

This repository uses a single-context domain-documentation layout.

## Before exploring

Read the following when they exist and are relevant to the area being changed:

- `CONTEXT.md` at the repository root
- `docs/adr/` for architectural decisions

If these files do not exist, proceed silently. Create them only when domain terminology or architectural decisions need to be recorded.

## Expected layout

```text
/
├── CONTEXT.md
├── docs/
│   └── adr/
└── src/
```

Use the vocabulary defined in `CONTEXT.md` when it exists. If proposed work contradicts an ADR, surface the conflict explicitly rather than silently overriding it.
