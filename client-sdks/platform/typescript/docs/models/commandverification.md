# CommandVerification

Verification outcome of an operation command; null for commands that are not operations

## Example Usage

```typescript
import { CommandVerification } from "@alienplatform/platform-api/models";

let value: CommandVerification = {
  state: "not-required",
  reason: "<value>",
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `state`                                                                  | [models.CommandVerificationState](../models/commandverificationstate.md) | :heavy_check_mark:                                                       | Verification outcome of an operation command.                            |
| `reason`                                                                 | *string*                                                                 | :heavy_check_mark:                                                       | Present for 'failed' and 'skipped'.                                      |
