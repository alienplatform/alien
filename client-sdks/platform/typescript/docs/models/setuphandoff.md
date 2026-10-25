# SetupHandoff

Use await-registration to reserve a pending deployment before installing and registering its infrastructure.

## Example Usage

```typescript
import { SetupHandoff } from "@alienplatform/platform-api/models";

let value: SetupHandoff = "immediate";
```

## Values

```typescript
"immediate" | "await-registration"
```