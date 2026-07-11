# InitialDesiredRelease

Desired-release selection for a new deployment. Use none to register an environment without initially requesting a release; later updates can assign one.

## Example Usage

```typescript
import { InitialDesiredRelease } from "@alienplatform/platform-api/models";

let value: InitialDesiredRelease = "active";
```

## Values

```typescript
"active" | "none"
```