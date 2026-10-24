---
"@alienplatform/platform-api": major
---

Regenerate the TypeScript platform API SDK with installation resume and check methods, AWS management External ID fields, and the latest API models and operations.

The API no longer exposes the billing audit-log route, so `billing.listAuditLog` and its generated models, functions, and React Query adapters are removed. Use `billing.getEntitlements` to read current entitlements or `billing.sync` to refresh them.
