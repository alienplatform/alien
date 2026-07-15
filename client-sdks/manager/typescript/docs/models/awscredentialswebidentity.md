# AwsCredentialsWebIdentity

Web Identity Token for OIDC authentication

## Example Usage

```typescript
import { AwsCredentialsWebIdentity } from "@alienplatform/manager-api/models";

let value: AwsCredentialsWebIdentity = {
  config: {
    roleArn: "<value>",
    webIdentityTokenFile: "<value>",
  },
  type: "webIdentity",
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `config`                                                         | [models.AwsWebIdentityConfig](../models/awswebidentityconfig.md) | :heavy_check_mark:                                               | Configuration for AWS Web Identity Token authentication          |
| `type`                                                           | *"webIdentity"*                                                  | :heavy_check_mark:                                               | N/A                                                              |