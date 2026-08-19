# TargetDeploymentGcpImages

GCP Horizon machine image entry.

## Example Usage

```typescript
import { TargetDeploymentGcpImages } from "@alienplatform/platform-api/models";

let value: TargetDeploymentGcpImages = {
  sourceImage: "<value>",
};
```

## Fields

| Field                                       | Type                                        | Required                                    | Description                                 |
| ------------------------------------------- | ------------------------------------------- | ------------------------------------------- | ------------------------------------------- |
| `sourceImage`                               | *string*                                    | :heavy_check_mark:                          | Source image self link or image-family URL. |