# DataDeploymentDeletionRequested

## Example Usage

```typescript
import { DataDeploymentDeletionRequested } from "@alienplatform/platform-api/models";

let value: DataDeploymentDeletionRequested = {
  deploymentId: "<id>",
  type: "DeploymentDeletionRequested",
};
```

## Fields

| Field                           | Type                            | Required                        | Description                     |
| ------------------------------- | ------------------------------- | ------------------------------- | ------------------------------- |
| `deploymentId`                  | *string*                        | :heavy_check_mark:              | ID of the deployment            |
| `type`                          | *"DeploymentDeletionRequested"* | :heavy_check_mark:              | N/A                             |