# Source

Import source configuration

## Example Usage

```typescript
import { Source } from "@aliendotdev/platform-api/models";

let value: Source = {
  type: "cloudformation",
  stackName: "<value>",
  region: "<value>",
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `type`                                                                         | [models.ImportDeploymentRequestType](../models/importdeploymentrequesttype.md) | :heavy_check_mark:                                                             | N/A                                                                            |
| `stackName`                                                                    | *string*                                                                       | :heavy_check_mark:                                                             | CloudFormation stack name to import                                            |
| `region`                                                                       | *string*                                                                       | :heavy_check_mark:                                                             | AWS region where the stack exists                                              |