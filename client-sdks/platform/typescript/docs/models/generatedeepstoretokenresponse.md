# GenerateDeepstoreTokenResponse

## Example Usage

```typescript
import { GenerateDeepstoreTokenResponse } from "@alienplatform/platform-api/models";

let value: GenerateDeepstoreTokenResponse = {
  accessToken: "<value>",
  expiresIn: 8619.26,
  tokenType: "Bearer",
  databaseId: "<id>",
  controlPlaneUrl: "https://numb-luck.org",
  authProxyUrl: "https://good-natured-pocket-watch.com/",
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `accessToken`                                                                                          | *string*                                                                                               | :heavy_check_mark:                                                                                     | JWT token for authenticating with DeepStore                                                            |
| `expiresIn`                                                                                            | *number*                                                                                               | :heavy_check_mark:                                                                                     | Token lifetime in seconds                                                                              |
| `tokenType`                                                                                            | [models.GenerateDeepstoreTokenResponseTokenType](../models/generatedeepstoretokenresponsetokentype.md) | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `databaseId`                                                                                           | *string*                                                                                               | :heavy_check_mark:                                                                                     | DeepStore database ID for this manager                                                                 |
| `controlPlaneUrl`                                                                                      | *string*                                                                                               | :heavy_check_mark:                                                                                     | DeepStore control plane URL (for split discovery)                                                      |
| `authProxyUrl`                                                                                         | *string*                                                                                               | :heavy_check_mark:                                                                                     | Manager URL acting as DeepStore auth proxy (for data plane queries)                                    |