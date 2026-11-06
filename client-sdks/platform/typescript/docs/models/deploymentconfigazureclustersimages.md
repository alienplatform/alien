# DeploymentConfigAzureClustersImages

Azure machine image entry.

## Example Usage

```typescript
import { DeploymentConfigAzureClustersImages } from "@alienplatform/platform-api/models";

let value: DeploymentConfigAzureClustersImages = {
  imageVersionId: "<id>",
};
```

## Fields

| Field                                   | Type                                    | Required                                | Description                             |
| --------------------------------------- | --------------------------------------- | --------------------------------------- | --------------------------------------- |
| `imageVersionId`                        | *string*                                | :heavy_check_mark:                      | Azure Compute Gallery image version ID. |