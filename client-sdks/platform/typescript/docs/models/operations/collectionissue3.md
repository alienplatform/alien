# CollectionIssue3

## Example Usage

```typescript
import { CollectionIssue3 } from "@alienplatform/platform-api/models/operations";

let value: CollectionIssue3 = {
  message: "<value>",
  reason: "collection-failed",
  severity: "info",
  source: "<value>",
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `message`                                                                                                      | *string*                                                                                                       | :heavy_check_mark:                                                                                             | N/A                                                                                                            |
| `reason`                                                                                                       | [operations.GetResourceDeploymentDetailReason3](../../models/operations/getresourcedeploymentdetailreason3.md) | :heavy_check_mark:                                                                                             | N/A                                                                                                            |
| `severity`                                                                                                     | [operations.CollectionIssueSeverity3](../../models/operations/collectionissueseverity3.md)                     | :heavy_check_mark:                                                                                             | N/A                                                                                                            |
| `source`                                                                                                       | *string*                                                                                                       | :heavy_check_mark:                                                                                             | N/A                                                                                                            |