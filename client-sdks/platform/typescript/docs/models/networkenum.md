# NetworkEnum

Optional network mode for the private-manager setup. Defaults to create for production isolation; default uses the provider default network for faster dev/test setup.

## Example Usage

```typescript
import { NetworkEnum } from "@alienplatform/platform-api/models";

let value: NetworkEnum = "default";
```

## Values

```typescript
"create" | "default"
```