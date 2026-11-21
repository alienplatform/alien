# VerifyOperationCheckResponseOutcome

verified: success condition met. not-yet: keep polling. failed: the poll operation itself reported failure. skipped: the operation has no verification spec, the write result carries none of the fields verification needs (the caller never opted in for this invocation), or the deployment runs on a Private Alien Instance, whose results Alien does not read.

## Example Usage

```typescript
import { VerifyOperationCheckResponseOutcome } from "@alienplatform/platform-api/models";

let value: VerifyOperationCheckResponseOutcome = "not-yet";
```

## Values

```typescript
"verified" | "not-yet" | "failed" | "skipped"
```