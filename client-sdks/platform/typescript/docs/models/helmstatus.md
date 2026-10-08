# HelmStatus

Status of a package build

## Example Usage

```typescript
import { HelmStatus } from "@alienplatform/platform-api/models";

let value: HelmStatus = "canceled";
```

## Values

```typescript
"pending" | "building" | "ready" | "failed" | "canceled"
```