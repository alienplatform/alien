# CreateAccessRequestResponse

The created access request.

## Example Usage

```typescript
import { CreateAccessRequestResponse } from "@alienplatform/platform-api/models/operations";

let value: CreateAccessRequestResponse = {
  id: "<id>",
  deploymentId: "<id>",
  remediationPlanId: "<id>",
  title: "<value>",
  reason: "<value>",
  commands: [
    {
      command: "kubernetes/get-pods",
      summary: "List pods in the ingestion namespace",
    },
  ],
  status: "rejected",
  approvedUntil: "<value>",
};
```

## Fields

| Field                                                                                                            | Type                                                                                                             | Required                                                                                                         | Description                                                                                                      |
| ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `id`                                                                                                             | *string*                                                                                                         | :heavy_check_mark:                                                                                               | N/A                                                                                                              |
| `deploymentId`                                                                                                   | *string*                                                                                                         | :heavy_check_mark:                                                                                               | N/A                                                                                                              |
| `remediationPlanId`                                                                                              | *string*                                                                                                         | :heavy_check_mark:                                                                                               | N/A                                                                                                              |
| `title`                                                                                                          | *string*                                                                                                         | :heavy_check_mark:                                                                                               | N/A                                                                                                              |
| `reason`                                                                                                         | *string*                                                                                                         | :heavy_check_mark:                                                                                               | N/A                                                                                                              |
| `commands`                                                                                                       | [operations.CreateAccessRequestCommandResponse](../../models/operations/createaccessrequestcommandresponse.md)[] | :heavy_check_mark:                                                                                               | N/A                                                                                                              |
| `status`                                                                                                         | [operations.CreateAccessRequestStatus](../../models/operations/createaccessrequeststatus.md)                     | :heavy_check_mark:                                                                                               | N/A                                                                                                              |
| `approvedUntil`                                                                                                  | *string*                                                                                                         | :heavy_check_mark:                                                                                               | N/A                                                                                                              |