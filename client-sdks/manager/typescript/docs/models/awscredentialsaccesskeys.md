# AwsCredentialsAccessKeys

Static direct access keys.

## Example Usage

```typescript
import { AwsCredentialsAccessKeys } from "@alienplatform/manager-api/models";

let value: AwsCredentialsAccessKeys = {
  accessKeyId: "<id>",
  secretAccessKey: "<value>",
  type: "accessKeys",
};
```

## Fields

| Field                      | Type                       | Required                   | Description                |
| -------------------------- | -------------------------- | -------------------------- | -------------------------- |
| `accessKeyId`              | *string*                   | :heavy_check_mark:         | AWS Access Key ID          |
| `secretAccessKey`          | *string*                   | :heavy_check_mark:         | AWS Secret Access Key      |
| `sessionToken`             | *string*                   | :heavy_minus_sign:         | Optional AWS Session Token |
| `type`                     | *"accessKeys"*             | :heavy_check_mark:         | N/A                        |