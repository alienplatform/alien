# RemoteAwsCredentialsSessionCredentials

Temporary AWS session credentials with an authoritative expiry.

## Example Usage

```typescript
import { RemoteAwsCredentialsSessionCredentials } from "@alienplatform/manager-api/models";

let value: RemoteAwsCredentialsSessionCredentials = {
  accessKeyId: "<id>",
  expiresAt: "1754675774082",
  secretAccessKey: "<value>",
  sessionToken: "<value>",
  type: "sessionCredentials",
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `accessKeyId`                                                            | *string*                                                                 | :heavy_check_mark:                                                       | AWS access key id.                                                       |
| `expiresAt`                                                              | *string*                                                                 | :heavy_check_mark:                                                       | Provider-reported credential expiry.                                     |
| `secretAccessKey`                                                        | *string*                                                                 | :heavy_check_mark:                                                       | AWS secret access key.                                                   |
| `sessionToken`                                                           | *string*                                                                 | :heavy_check_mark:                                                       | AWS session token.                                                       |
| `type`                                                                   | [models.RemoteAwsCredentialsType](../models/remoteawscredentialstype.md) | :heavy_check_mark:                                                       | N/A                                                                      |