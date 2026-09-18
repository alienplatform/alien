# ListDebugSessionsProvider

Filter by cloud provider. Joins against the parent deployment.

## Example Usage

```typescript
import { ListDebugSessionsProvider } from "@alienplatform/platform-api/models/operations";

let value: ListDebugSessionsProvider = "azure";
```

## Values

```typescript
"aws" | "gcp" | "azure" | "kubernetes" | "machines" | "local" | "test"
```