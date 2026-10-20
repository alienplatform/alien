# Relation

`desired`: the release the deployment is updating to. `current`: the release it last installed. `other`: any other release of the project, such as one whose update failed.

## Example Usage

```typescript
import { Relation } from "@alienplatform/platform-api/models/operations";

let value: Relation = "current";
```

## Values

```typescript
"desired" | "current" | "other"
```