# StackImportResponse

Response body returned after a stack import.

## Example Usage

```typescript
import { StackImportResponse } from "@alienplatform/manager-api/models";

let value: StackImportResponse = {
  deploymentId: "<id>",
  stackSettings: {},
  stackState: {
    platform: "azure",
    resourcePrefix: "<value>",
    resources: {
      "key": {
        config: {
          id: "<id>",
          type: "worker",
        },
        dependencies: [
          {
            id: "<id>",
            type: "worker",
          },
        ],
        error: {
          code: "NOT_FOUND",
          internal: false,
          message: "Item not found.",
        },
        outputs: {
          type: "worker",
        },
        previousConfig: {
          id: "<id>",
          type: "worker",
        },
        status: "provision-failed",
        type: "<value>",
      },
    },
  },
};
```

## Fields

| Field                                                                                                                                                                                                                                                                                                                                                                                                            | Type                                                                                                                                                                                                                                                                                                                                                                                                             | Required                                                                                                                                                                                                                                                                                                                                                                                                         | Description                                                                                                                                                                                                                                                                                                                                                                                                      |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `deploymentId`                                                                                                                                                                                                                                                                                                                                                                                                   | *string*                                                                                                                                                                                                                                                                                                                                                                                                         | :heavy_check_mark:                                                                                                                                                                                                                                                                                                                                                                                               | Deployment created.                                                                                                                                                                                                                                                                                                                                                                                              |
| `stackSettings`                                                                                                                                                                                                                                                                                                                                                                                                  | [models.StackSettings](../models/stacksettings.md)                                                                                                                                                                                                                                                                                                                                                               | :heavy_check_mark:                                                                                                                                                                                                                                                                                                                                                                                               | User-customizable deployment settings specified at deploy time.<br/><br/>These settings are provided by the customer via CloudFormation parameters,<br/>Terraform attributes, CLI flags, or Helm values. They customize how the<br/>deployment runs and what capabilities are enabled.<br/><br/>**Key distinction**: StackSettings is user-customizable, while ManagementConfig<br/>is platform-derived (from the Manager's ServiceAccount). |
| `stackState`                                                                                                                                                                                                                                                                                                                                                                                                     | [models.StackState](../models/stackstate.md)                                                                                                                                                                                                                                                                                                                                                                     | :heavy_check_mark:                                                                                                                                                                                                                                                                                                                                                                                               | Represents the collective state of all resources in a stack, including platform and pending actions.                                                                                                                                                                                                                                                                                                             |