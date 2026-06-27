# ReleaseInfoSetupMethod

Setup methods that can collect deployer-provided input values.

## Example Usage

```typescript
import { ReleaseInfoSetupMethod } from "@alienplatform/platform-api/models";

let value: ReleaseInfoSetupMethod = "cloud-formation";
```

## Values

```typescript
"cli" | "terraform" | "cloud-formation" | "helm" | "google-oauth"
```