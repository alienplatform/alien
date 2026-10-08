# CreateCommandResponse

## Example Usage

```typescript
import { CreateCommandResponse } from "@alienplatform/platform-api/models";

let value: CreateCommandResponse = {
  id: "cmd_2sxjXxvOYct7IohT3ukliAzf",
  projectId: "<id>",
  deploymentModel: "push",
  target: {
    resourceId: "<id>",
    resourceType: "container",
  },
  deliveryMode: "pull",
  operationResultContractPersisted: false,
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      | Example                                                                                          |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `id`                                                                                             | *string*                                                                                         | :heavy_check_mark:                                                                               | Unique identifier for the command.                                                               | cmd_2sxjXxvOYct7IohT3ukliAzf                                                                     |
| `projectId`                                                                                      | *string*                                                                                         | :heavy_check_mark:                                                                               | Project ID (for manager to use in routing)                                                       |                                                                                                  |
| `deploymentModel`                                                                                | [models.CreateCommandResponseDeploymentModel](../models/createcommandresponsedeploymentmodel.md) | :heavy_check_mark:                                                                               | How to dispatch the command                                                                      |                                                                                                  |
| `target`                                                                                         | [models.CreateCommandResponseTarget](../models/createcommandresponsetarget.md)                   | :heavy_check_mark:                                                                               | Resource the command is addressed to                                                             |                                                                                                  |
| `deliveryMode`                                                                                   | [models.CreateCommandResponseDeliveryMode](../models/createcommandresponsedeliverymode.md)       | :heavy_check_mark:                                                                               | How the command is delivered to its target                                                       |                                                                                                  |
| `operationResultContractPersisted`                                                               | *boolean*                                                                                        | :heavy_check_mark:                                                                               | Whether an operation result contract was stored atomically with this command                     |                                                                                                  |