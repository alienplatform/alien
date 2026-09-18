# GetProjectSandboxMetricsCustomer

## Example Usage

```typescript
import { GetProjectSandboxMetricsCustomer } from "@alienplatform/platform-api/models/operations";

let value: GetProjectSandboxMetricsCustomer = {
  deploymentGroupId: "<id>",
  externalId: "<id>",
  sessions: 302307,
  commands: 118828,
  lastSeenAt: new Date("2024-09-04T12:37:38.251Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `deploymentGroupId`                                                                           | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `externalId`                                                                                  | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `sessions`                                                                                    | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `commands`                                                                                    | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `lastSeenAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |