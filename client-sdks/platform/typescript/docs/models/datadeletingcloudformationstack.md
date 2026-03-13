# DataDeletingCloudFormationStack

## Example Usage

```typescript
import { DataDeletingCloudFormationStack } from "@aliendotdev/platform-api/models";

let value: DataDeletingCloudFormationStack = {
  cfnStackName: "<value>",
  currentStatus: "<value>",
  type: "DeletingCloudFormationStack",
};
```

## Fields

| Field                            | Type                             | Required                         | Description                      |
| -------------------------------- | -------------------------------- | -------------------------------- | -------------------------------- |
| `cfnStackName`                   | *string*                         | :heavy_check_mark:               | Name of the CloudFormation stack |
| `currentStatus`                  | *string*                         | :heavy_check_mark:               | Current stack status             |
| `type`                           | *"DeletingCloudFormationStack"*  | :heavy_check_mark:               | N/A                              |