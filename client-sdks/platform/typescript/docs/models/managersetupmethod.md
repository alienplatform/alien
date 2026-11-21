# ManagerSetupMethod

How this Private Alien Instance was set up. Absent for Alien-hosted managers.

## Example Usage

```typescript
import { ManagerSetupMethod } from "@alienplatform/platform-api/models";

let value: ManagerSetupMethod = "helm";
```

## Values

```typescript
"cloudformation" | "google-oauth" | "terraform" | "helm" | "cli" | "manual"
```