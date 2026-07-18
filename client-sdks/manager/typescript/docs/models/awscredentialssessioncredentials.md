# AwsCredentialsSessionCredentials

Temporary AWS session credentials with an expiration time.

## Example Usage

```typescript
import { AwsCredentialsSessionCredentials } from "@alienplatform/manager-api/models";

let value: AwsCredentialsSessionCredentials = {
  accessKeyId: "<id>",
  expiresAt: "1748259840067",
  secretAccessKey: "<value>",
  sessionToken: "<value>",
  type: "sessionCredentials",
};
```

## Fields

| Field                                         | Type                                          | Required                                      | Description                                   |
| --------------------------------------------- | --------------------------------------------- | --------------------------------------------- | --------------------------------------------- |
| `accessKeyId`                                 | *string*                                      | :heavy_check_mark:                            | AWS Access Key ID                             |
| `expiresAt`                                   | *string*                                      | :heavy_check_mark:                            | Credential expiration as an RFC3339 timestamp |
| `secretAccessKey`                             | *string*                                      | :heavy_check_mark:                            | AWS Secret Access Key                         |
| `sessionToken`                                | *string*                                      | :heavy_check_mark:                            | AWS Session Token                             |
| `type`                                        | *"sessionCredentials"*                        | :heavy_check_mark:                            | N/A                                           |