# GcpCredentialsExternalAccount

Use an external account credential configuration.

## Example Usage

```typescript
import { GcpCredentialsExternalAccount } from "@alienplatform/manager-api/models";

let value: GcpCredentialsExternalAccount = {
  audience: "<value>",
  credentialSourceFile: "<value>",
  subjectTokenType: "<value>",
  tokenUrl: "https://everlasting-platter.net",
  type: "externalAccount",
};
```

## Fields

| Field                                       | Type                                        | Required                                    | Description                                 |
| ------------------------------------------- | ------------------------------------------- | ------------------------------------------- | ------------------------------------------- |
| `audience`                                  | *string*                                    | :heavy_check_mark:                          | Workload identity audience.                 |
| `credentialSourceFile`                      | *string*                                    | :heavy_check_mark:                          | Path to the subject token file.             |
| `serviceAccountImpersonationUrl`            | *string*                                    | :heavy_minus_sign:                          | Optional service account impersonation URL. |
| `subjectTokenType`                          | *string*                                    | :heavy_check_mark:                          | Subject token type for STS token exchange.  |
| `tokenUrl`                                  | *string*                                    | :heavy_check_mark:                          | STS token exchange URL.                     |
| `type`                                      | *"externalAccount"*                         | :heavy_check_mark:                          | N/A                                         |