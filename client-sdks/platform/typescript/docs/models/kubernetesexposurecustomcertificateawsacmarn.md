# KubernetesExposureCustomCertificateAwsAcmArn

## Example Usage

```typescript
import { KubernetesExposureCustomCertificateAwsAcmArn } from "@alienplatform/platform-api/models";

let value: KubernetesExposureCustomCertificateAwsAcmArn = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

## Fields

| Field                         | Type                          | Required                      | Description                   |
| ----------------------------- | ----------------------------- | ----------------------------- | ----------------------------- |
| `certificateArn`              | *string*                      | :heavy_check_mark:            | Existing ACM certificate ARN. |
| `mode`                        | *"awsAcmArn"*                 | :heavy_check_mark:            | N/A                           |