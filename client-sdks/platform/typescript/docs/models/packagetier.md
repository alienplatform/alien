# PackageTier

Effective risk tier of an enabled operation in a Helm build snapshot.

## Example Usage

```typescript
import { PackageTier } from "@alienplatform/platform-api/models";

let value: PackageTier = "read-only";
```

## Values

```typescript
"read-only" | "mutating" | "destructive"
```