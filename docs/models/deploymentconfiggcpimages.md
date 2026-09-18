# DeploymentConfigGcpImages

GCP Horizon machine image entry.

## Example Usage

```typescript
import { DeploymentConfigGcpImages } from "@alienplatform/platform-api/models";

let value: DeploymentConfigGcpImages = {
  sourceImage: "<value>",
};
```

## Fields

| Field                                       | Type                                        | Required                                    | Description                                 |
| ------------------------------------------- | ------------------------------------------- | ------------------------------------------- | ------------------------------------------- |
| `sourceImage`                               | *string*                                    | :heavy_check_mark:                          | Source image self link or image-family URL. |