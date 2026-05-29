# CloudFormationCallbackRequestCertificateAwsAcmArn2

## Example Usage

```typescript
import { CloudFormationCallbackRequestCertificateAwsAcmArn2 } from "@alienplatform/platform-api/models";

let value: CloudFormationCallbackRequestCertificateAwsAcmArn2 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

## Fields

| Field                         | Type                          | Required                      | Description                   |
| ----------------------------- | ----------------------------- | ----------------------------- | ----------------------------- |
| `certificateArn`              | *string*                      | :heavy_check_mark:            | Existing ACM certificate ARN. |
| `mode`                        | *"awsAcmArn"*                 | :heavy_check_mark:            | N/A                           |