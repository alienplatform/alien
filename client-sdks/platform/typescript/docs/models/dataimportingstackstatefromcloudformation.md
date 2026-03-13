# DataImportingStackStateFromCloudFormation

## Example Usage

```typescript
import { DataImportingStackStateFromCloudFormation } from "@aliendotdev/platform-api/models";

let value: DataImportingStackStateFromCloudFormation = {
  cfnStackName: "<value>",
  type: "ImportingStackStateFromCloudFormation",
};
```

## Fields

| Field                                     | Type                                      | Required                                  | Description                               |
| ----------------------------------------- | ----------------------------------------- | ----------------------------------------- | ----------------------------------------- |
| `cfnStackName`                            | *string*                                  | :heavy_check_mark:                        | Name of the CloudFormation stack          |
| `type`                                    | *"ImportingStackStateFromCloudFormation"* | :heavy_check_mark:                        | N/A                                       |