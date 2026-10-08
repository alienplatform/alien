# VerifyOperationCheckRequest

## Example Usage

```typescript
import { VerifyOperationCheckRequest } from "@alienplatform/platform-api/models";

let value: VerifyOperationCheckRequest = {
  deploymentId: "<id>",
  commandId: "cmd_2sxjXxvOYct7IohT3ukliAzf",
};
```

## Fields

| Field                                                                                                     | Type                                                                                                      | Required                                                                                                  | Description                                                                                               | Example                                                                                                   |
| --------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| `deploymentId`                                                                                            | *string*                                                                                                  | :heavy_check_mark:                                                                                        | Deployment the original write command ran against.                                                        |                                                                                                           |
| `commandId`                                                                                               | *string*                                                                                                  | :heavy_check_mark:                                                                                        | Original operation command whose stored result and dispatch-time verification contract are authoritative. | cmd_2sxjXxvOYct7IohT3ukliAzf                                                                              |