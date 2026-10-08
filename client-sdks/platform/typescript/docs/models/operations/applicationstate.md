# ApplicationState

How the application release the Operator reports compares with the desired release. Alien never applies the change; the customer applies it through their own release process.

## Example Usage

```typescript
import { ApplicationState } from "@alienplatform/platform-api/models/operations";

let value: ApplicationState = "no-desired";
```

## Values

```typescript
"no-desired" | "up-to-date" | "update-required" | "unknown" | "failed"
```