# DeploymentConfigAzureImages

Azure Horizon machine image entry.

## Example Usage

```typescript
import { DeploymentConfigAzureImages } from "@alienplatform/platform-api/models";

let value: DeploymentConfigAzureImages = {
  imageVersionId: "<id>",
};
```

## Fields

| Field                                   | Type                                    | Required                                | Description                             |
| --------------------------------------- | --------------------------------------- | --------------------------------------- | --------------------------------------- |
| `imageVersionId`                        | *string*                                | :heavy_check_mark:                      | Azure Compute Gallery image version ID. |