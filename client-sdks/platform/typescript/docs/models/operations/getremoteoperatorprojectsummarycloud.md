# GetRemoteOperatorProjectSummaryCloud

Cloud permissions required to execute this operation.

## Example Usage

```typescript
import { GetRemoteOperatorProjectSummaryCloud } from "@alienplatform/platform-api/models/operations";

let value: GetRemoteOperatorProjectSummaryCloud = {
  azure: [],
  aws: [
    {
      effect: "Deny",
      actions: [
        "<value 1>",
      ],
      resources: [
        "<value 1>",
      ],
      condition: {
        "key": {
          "key": "<value>",
          "key1": "<value>",
        },
        "key1": {
          "key": "<value>",
        },
      },
      reason: "<value>",
    },
  ],
  gcp: [],
};
```

## Fields

| Field                                                                                                                                                                         | Type                                                                                                                                                                          | Required                                                                                                                                                                      | Description                                                                                                                                                                   |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `azure`                                                                                                                                                                       | *string*[]                                                                                                                                                                    | :heavy_check_mark:                                                                                                                                                            | Always empty: Azure resource operations are not supported yet because no Azure resource permission has been reviewed for operations. Kubernetes API permissions are separate. |
| `aws`                                                                                                                                                                         | [operations.Aw](../../models/operations/aw.md)[]                                                                                                                              | :heavy_check_mark:                                                                                                                                                            | N/A                                                                                                                                                                           |
| `gcp`                                                                                                                                                                         | [operations.GetRemoteOperatorProjectSummaryGcp](../../models/operations/getremoteoperatorprojectsummarygcp.md)[]                                                              | :heavy_check_mark:                                                                                                                                                            | N/A                                                                                                                                                                           |