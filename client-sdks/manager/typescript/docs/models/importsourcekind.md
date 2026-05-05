# ImportSourceKind

Distribution source that produced an import request. Observability label
only — the manager does not branch on this value, and any new deployment
pathway can omit it without affecting import behavior.

## Example Usage

```typescript
import { ImportSourceKind } from "@alienplatform/manager-api/models";

let value: ImportSourceKind = "terraform";
```

## Values

```typescript
"cloud-formation" | "terraform" | "helm"
```