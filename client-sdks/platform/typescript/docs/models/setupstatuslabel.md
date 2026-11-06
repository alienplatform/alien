# SetupStatusLabel

Display-only progress of a pending deployment's saved setup, as the deployment detail derives it from setup metadata. Null when the deployment is not pending or has no saved setup.

## Example Usage

```typescript
import { SetupStatusLabel } from "@alienplatform/platform-api/models";

let value: SetupStatusLabel = "Setup rolled back";
```

## Values

```typescript
"Awaiting setup" | "Setting up" | "Setup rolled back"
```