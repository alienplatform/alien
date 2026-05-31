# KubernetesCertificateModeAwsAcmArn

Customer-provided AWS ACM certificate ARN.

## Example Usage

```typescript
import { KubernetesCertificateModeAwsAcmArn } from "@alienplatform/manager-api/models";

let value: KubernetesCertificateModeAwsAcmArn = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

## Fields

| Field                         | Type                          | Required                      | Description                   |
| ----------------------------- | ----------------------------- | ----------------------------- | ----------------------------- |
| `certificateArn`              | *string*                      | :heavy_check_mark:            | Existing ACM certificate ARN. |
| `mode`                        | *"awsAcmArn"*                 | :heavy_check_mark:            | N/A                           |