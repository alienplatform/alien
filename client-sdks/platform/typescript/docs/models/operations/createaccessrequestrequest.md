# CreateAccessRequestRequest

## Example Usage

```typescript
import { CreateAccessRequestRequest } from "@alienplatform/platform-api/models/operations";

let value: CreateAccessRequestRequest = {
  deploymentId: "<id>",
  remediationPlanId: "<id>",
  title: "<value>",
  commands: [
    {
      command: "kubernetes/get-pods",
      summary: "List pods in the ingestion namespace",
      params: {
        "pod": "ingester-p4kwm",
      },
    },
  ],
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