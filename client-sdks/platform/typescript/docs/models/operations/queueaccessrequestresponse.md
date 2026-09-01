# QueueAccessRequestResponse

The queued access request, with the customer approve command.

## Example Usage

```typescript
import { QueueAccessRequestResponse } from "@alienplatform/platform-api/models/operations";

let value: QueueAccessRequestResponse = {
  id: "<id>",
  deploymentId: "<id>",
  remediationPlanId: "<id>",
  title: "<value>",
  reason: "<value>",
  commands: [
    {
      command: "kubernetes/get-pods",
      summary: "List pods in the ingestion namespace",
      params: {
        "pod": "ingester-p4kwm",
      },
    },
  ],
  status: "customer-approved",
  approvedUntil: "<value>",
  kubectlApprove: "<value>",
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `id`                                                                                           | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `deploymentId`                                                                                 | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `remediationPlanId`                                                                            | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `title`                                                                                        | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `reason`                                                                                       | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `commands`                                                                                     | [operations.QueueAccessRequestCommand](../../models/operations/queueaccessrequestcommand.md)[] | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `status`                                                                                       | [operations.QueueAccessRequestStatus](../../models/operations/queueaccessrequeststatus.md)     | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `approvedUntil`                                                                                | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `kubectlApprove`                                                                               | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |