# ManagerCloud

Cloud where the private manager is hosted. Null for Alien-hosted managers.

## Example Usage

```typescript
import { ManagerCloud } from "@alienplatform/platform-api/models";

let value: ManagerCloud = "kubernetes";
```

## Values

```typescript
"aws" | "gcp" | "azure" | "kubernetes" | "local" | "test"
```