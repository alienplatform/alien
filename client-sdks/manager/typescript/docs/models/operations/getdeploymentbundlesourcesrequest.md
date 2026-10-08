# GetDeploymentBundleSourcesRequest

## Example Usage

```typescript
import { GetDeploymentBundleSourcesRequest } from "@alienplatform/manager-api/models/operations";

let value: GetDeploymentBundleSourcesRequest = {
  id: "<id>",
  releaseId: "<id>",
};
```

## Fields

| Field                       | Type                        | Required                    | Description                 |
| --------------------------- | --------------------------- | --------------------------- | --------------------------- |
| `id`                        | *string*                    | :heavy_check_mark:          | Deployment ID               |
| `releaseId`                 | *string*                    | :heavy_check_mark:          | Release whose chart to use. |