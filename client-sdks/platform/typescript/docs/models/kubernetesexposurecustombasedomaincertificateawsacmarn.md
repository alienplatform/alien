# KubernetesExposureCustomBaseDomainCertificateAwsAcmArn

## Example Usage

```typescript
import { KubernetesExposureCustomBaseDomainCertificateAwsAcmArn } from "@alienplatform/platform-api/models";

let value: KubernetesExposureCustomBaseDomainCertificateAwsAcmArn = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

## Fields

| Field                         | Type                          | Required                      | Description                   |
| ----------------------------- | ----------------------------- | ----------------------------- | ----------------------------- |
| `certificateArn`              | *string*                      | :heavy_check_mark:            | Existing ACM certificate ARN. |
| `mode`                        | *"awsAcmArn"*                 | :heavy_check_mark:            | N/A                           |