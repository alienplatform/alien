# TerraformStatus

Status of a package build

## Example Usage

```typescript
import { TerraformStatus } from "@aliendotdev/platform-api/models";

let value: TerraformStatus = "canceled";
```

## Values

```typescript
"pending" | "building" | "ready" | "failed" | "canceled"
```