# InitializeResponse

## Example Usage

```typescript
import { InitializeResponse } from "@alienplatform/manager-api/models";

let value: InitializeResponse = {
  deploymentId: "<id>",
  deploymentModel: "pull",
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `deploymentId`                                                         | *string*                                                               | :heavy_check_mark:                                                     | N/A                                                                    |
| `deploymentModel`                                                      | [models.DeploymentModel](../models/deploymentmodel.md)                 | :heavy_check_mark:                                                     | Deployment model: how updates are delivered to the remote environment. |
| `token`                                                                | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |