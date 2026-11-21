# DeploymentEndpointHealthChangedEvent

## Example Usage

```typescript
import { DeploymentEndpointHealthChangedEvent } from "@alienplatform/platform-api/models";

let value: DeploymentEndpointHealthChangedEvent = {
  type: "DeploymentEndpointHealthChanged",
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  resourceId: "<id>",
  observedAt: new Date("2025-09-02T13:18:55.549Z"),
  targets: [],
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    | Example                                                                                                        |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `type`                                                                                                         | *"DeploymentEndpointHealthChanged"*                                                                            | :heavy_check_mark:                                                                                             | N/A                                                                                                            |                                                                                                                |
| `deploymentId`                                                                                                 | *string*                                                                                                       | :heavy_check_mark:                                                                                             | Unique identifier for the deployment.                                                                          | dep_0c29fq4a2yjb7kx3smwdgxlc                                                                                   |
| `resourceId`                                                                                                   | *string*                                                                                                       | :heavy_check_mark:                                                                                             | N/A                                                                                                            |                                                                                                                |
| `observedAt`                                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)                  | :heavy_check_mark:                                                                                             | N/A                                                                                                            |                                                                                                                |
| `targets`                                                                                                      | [models.DeploymentEndpointHealthChangedEventTarget](../models/deploymentendpointhealthchangedeventtarget.md)[] | :heavy_check_mark:                                                                                             | N/A                                                                                                            |                                                                                                                |