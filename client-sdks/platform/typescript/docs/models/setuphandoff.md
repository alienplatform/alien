# SetupHandoff

Use await-registration when CloudFormation will create the infrastructure and register this deployment.

## Example Usage

```typescript
import { SetupHandoff } from "@alienplatform/platform-api/models";

let value: SetupHandoff = "immediate";
```

## Values

```typescript
"immediate" | "await-registration"
```