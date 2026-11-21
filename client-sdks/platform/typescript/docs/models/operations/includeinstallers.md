# IncludeInstallers

Also list installer keys: the 24-hour keys minted for a setup link's prepared installers and portal sessions. They are left out by default; each stops working with its setup link. Honored for users and for workspace or project keys; other keys never list installer keys.

## Example Usage

```typescript
import { IncludeInstallers } from "@alienplatform/platform-api/models/operations";

let value: IncludeInstallers = "false";
```

## Values

```typescript
"true" | "false"
```