# GetRemoteOperatorProjectSummaryVerification

## Example Usage

```typescript
import { GetRemoteOperatorProjectSummaryVerification } from "@alienplatform/platform-api/models/operations";

let value: GetRemoteOperatorProjectSummaryVerification = {
  state: "not-required",
  attempts: 381103,
  maxAttempts: 707038,
  deadline: new Date("2024-01-13T22:24:01.360Z"),
  reason: null,
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `state`                                                                                       | [operations.VerificationState](../../models/operations/verificationstate.md)                  | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `attempts`                                                                                    | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `maxAttempts`                                                                                 | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `deadline`                                                                                    | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `reason`                                                                                      | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |