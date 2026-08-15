# SetupPackagesStatus

Whether at least one complete automated setup package is ready across all selected setup items.

## Example Usage

```typescript
import { SetupPackagesStatus } from "@alienplatform/platform-api/models";

let value: SetupPackagesStatus = "ready";
```

## Values

```typescript
"preparing" | "ready" | "failed"
```