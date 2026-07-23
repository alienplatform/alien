# EventDataImportingStackStateFromCloudFormation

## Example Usage

```typescript
import { EventDataImportingStackStateFromCloudFormation } from "@alienplatform/platform-api/models";

let value: EventDataImportingStackStateFromCloudFormation = {
  cfnStackName: "<value>",
  type: "ImportingStackStateFromCloudFormation",
};
```

## Fields

| Field                                     | Type                                      | Required                                  | Description                               |
| ----------------------------------------- | ----------------------------------------- | ----------------------------------------- | ----------------------------------------- |
| `cfnStackName`                            | *string*                                  | :heavy_check_mark:                        | Name of the CloudFormation stack          |
| `type`                                    | *"ImportingStackStateFromCloudFormation"* | :heavy_check_mark:                        | N/A                                       |
