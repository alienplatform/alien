# SyncReleaseRequestMode

Defaults to release. acknowledge-completed discards an exact terminal pull receipt after a lost response; it never releases newer work or grants a live lease.

## Example Usage

```typescript
import { SyncReleaseRequestMode } from "@alienplatform/platform-api/models";

let value: SyncReleaseRequestMode = "acknowledge-completed";
```

## Values

```typescript
"release" | "acknowledge-completed"
```
