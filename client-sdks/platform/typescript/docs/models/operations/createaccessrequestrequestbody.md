# CreateAccessRequestRequestBody

## Example Usage

```typescript
import { CreateAccessRequestRequestBody } from "@alienplatform/platform-api/models/operations";

let value: CreateAccessRequestRequestBody = {
  deploymentId: "<id>",
  remediationPlanId: "<id>",
  title: "<value>",
  commands: [],
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `deploymentId`                                                           | *string*                                                                 | :heavy_check_mark:                                                       | N/A                                                                      |
| `remediationPlanId`                                                      | *string*                                                                 | :heavy_check_mark:                                                       | N/A                                                                      |
| `title`                                                                  | *string*                                                                 | :heavy_check_mark:                                                       | N/A                                                                      |
| `reason`                                                                 | *string*                                                                 | :heavy_minus_sign:                                                       | N/A                                                                      |
| `commands`                                                               | [operations.CommandRequest](../../models/operations/commandrequest.md)[] | :heavy_check_mark:                                                       | N/A                                                                      |