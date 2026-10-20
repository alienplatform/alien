# DeploymentCheckRequestedEvent

## Example Usage

```typescript
import { DeploymentCheckRequestedEvent } from "@alienplatform/platform-api/models";

let value: DeploymentCheckRequestedEvent = {
  type: "DeploymentCheckRequested",
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
};
```

## Fields

| Field                                 | Type                                  | Required                              | Description                           | Example                               |
| ------------------------------------- | ------------------------------------- | ------------------------------------- | ------------------------------------- | ------------------------------------- |
| `type`                                | *"DeploymentCheckRequested"*          | :heavy_check_mark:                    | N/A                                   |                                       |
| `deploymentId`                        | *string*                              | :heavy_check_mark:                    | Unique identifier for the deployment. | dep_0c29fq4a2yjb7kx3smwdgxlc          |