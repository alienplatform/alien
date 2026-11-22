# TargetDeploymentAzureImages

Azure Horizon machine image entry.

## Example Usage

```typescript
import { TargetDeploymentAzureImages } from "@alienplatform/platform-api/models";

let value: TargetDeploymentAzureImages = {
  imageVersionId: "<id>",
};
```

## Fields

| Field                                   | Type                                    | Required                                | Description                             |
| --------------------------------------- | --------------------------------------- | --------------------------------------- | --------------------------------------- |
| `imageVersionId`                        | *string*                                | :heavy_check_mark:                      | Azure Compute Gallery image version ID. |