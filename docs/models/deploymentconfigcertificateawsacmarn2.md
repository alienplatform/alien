# DeploymentConfigCertificateAwsAcmArn2

## Example Usage

```typescript
import { DeploymentConfigCertificateAwsAcmArn2 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigCertificateAwsAcmArn2 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

## Fields

| Field                         | Type                          | Required                      | Description                   |
| ----------------------------- | ----------------------------- | ----------------------------- | ----------------------------- |
| `certificateArn`              | *string*                      | :heavy_check_mark:            | Existing ACM certificate ARN. |
| `mode`                        | *"awsAcmArn"*                 | :heavy_check_mark:            | N/A                           |