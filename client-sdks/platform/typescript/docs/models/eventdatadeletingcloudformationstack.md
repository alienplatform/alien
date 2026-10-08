# EventDataDeletingCloudFormationStack

## Example Usage

```typescript
import { EventDataDeletingCloudFormationStack } from "@alienplatform/platform-api/models";

let value: EventDataDeletingCloudFormationStack = {
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
