# EventListItemResponseDataDeployingCloudFormationStack

## Example Usage

```typescript
import { EventListItemResponseDataDeployingCloudFormationStack } from "@alienplatform/platform-api/models";

let value: EventListItemResponseDataDeployingCloudFormationStack = {
  cfnStackName: "<value>",
  currentStatus: "<value>",
  type: "DeployingCloudFormationStack",
};
```

## Fields

| Field                            | Type                             | Required                         | Description                      |
| -------------------------------- | -------------------------------- | -------------------------------- | -------------------------------- |
| `cfnStackName`                   | *string*                         | :heavy_check_mark:               | Name of the CloudFormation stack |
| `currentStatus`                  | *string*                         | :heavy_check_mark:               | Current stack status             |
| `type`                           | *"DeployingCloudFormationStack"* | :heavy_check_mark:               | N/A                              |
