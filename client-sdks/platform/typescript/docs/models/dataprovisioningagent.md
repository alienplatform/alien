# DataProvisioningAgent

## Example Usage

```typescript
import { DataProvisioningAgent } from "@alienplatform/platform-api/models";

let value: DataProvisioningAgent = {
  agentId: "<id>",
  releaseId: "<id>",
  type: "ProvisioningAgent",
};
```

## Fields

| Field                                         | Type                                          | Required                                      | Description                                   |
| --------------------------------------------- | --------------------------------------------- | --------------------------------------------- | --------------------------------------------- |
| `agentId`                                     | *string*                                      | :heavy_check_mark:                            | ID of the agent being provisioned             |
| `releaseId`                                   | *string*                                      | :heavy_check_mark:                            | ID of the release being deployed to the agent |
| `type`                                        | *"ProvisioningAgent"*                         | :heavy_check_mark:                            | N/A                                           |