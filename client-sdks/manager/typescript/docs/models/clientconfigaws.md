# ClientConfigAws

AWS client configuration

## Example Usage

```typescript
import { ClientConfigAws } from "@alienplatform/manager-api/models";

let value: ClientConfigAws = {
  accountId: "<id>",
  credentials: {
    accessKeyId: "<id>",
    secretAccessKey: "<value>",
    type: "accessKeys",
  },
  region: "<value>",
  platform: "aws",
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `accountId`                                                            | *string*                                                               | :heavy_check_mark:                                                     | The AWS Account ID.                                                    |
| `credentials`                                                          | *models.AwsCredentials*                                                | :heavy_check_mark:                                                     | Supported AWS authentication methods                                   |
| `region`                                                               | *string*                                                               | :heavy_check_mark:                                                     | The AWS region.                                                        |
| `serviceOverrides`                                                     | [models.AwsServiceOverrides](../models/awsserviceoverrides.md)         | :heavy_minus_sign:                                                     | N/A                                                                    |
| `platform`                                                             | [models.ClientConfigPlatformAws](../models/clientconfigplatformaws.md) | :heavy_check_mark:                                                     | N/A                                                                    |